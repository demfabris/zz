use zz_protocol::{AgentImage, MAX_AGENT_PROMPT_BYTES, MAX_AGENT_PROMPT_IMAGES};

pub fn validate_images(text: &str, images: &[AgentImage]) -> Result<(), String> {
    if images.len() > MAX_AGENT_PROMPT_IMAGES {
        return Err(format!(
            "Attach at most {MAX_AGENT_PROMPT_IMAGES} images per message."
        ));
    }
    let mut bytes = text.len();
    for image in images {
        validate_format(&image.format)?;
        if image.data.is_empty() {
            return Err("The selected image is empty.".into());
        }
        bytes = bytes.checked_add(image.data.len()).ok_or_else(size_error)?;
    }
    if bytes > MAX_AGENT_PROMPT_BYTES {
        return Err(size_error());
    }
    Ok(())
}

fn validate_format(format: &str) -> Result<(), String> {
    if matches!(format, "image/png" | "image/jpeg" | "image/webp") {
        Ok(())
    } else {
        Err("Choose a PNG, JPEG, or WebP image.".into())
    }
}

fn size_error() -> String {
    "The message and its images must total 6 MiB or less.".into()
}

#[cfg(target_family = "wasm")]
pub use browser::choose_images;

#[cfg(not(target_family = "wasm"))]
pub fn choose_images() -> std::future::Ready<Result<Vec<AgentImage>, String>> {
    std::future::ready(Err("Image selection is available in the browser.".into()))
}

#[cfg(target_family = "wasm")]
mod browser {
    use wasm_bindgen::{JsCast as _, closure::Closure};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Event, File, HtmlInputElement};

    use super::{AgentImage, MAX_AGENT_PROMPT_BYTES, MAX_AGENT_PROMPT_IMAGES};

    struct Picker {
        input: HtmlInputElement,
        completion: Closure<dyn FnMut(Event)>,
    }

    impl Picker {
        fn open() -> Result<(Self, async_channel::Receiver<bool>), String> {
            let document = web_sys::window()
                .and_then(|window| window.document())
                .ok_or("The browser document is unavailable.")?;
            let input = document
                .create_element("input")
                .map_err(|_| "Could not create the image picker.")?
                .dyn_into::<HtmlInputElement>()
                .map_err(|_| "Could not create the image picker.")?;
            input.set_type("file");
            input.set_accept("image/png,image/jpeg,image/webp");
            input.set_multiple(true);
            input.set_hidden(true);
            let (sender, receiver) = async_channel::bounded(1);
            let completion = Closure::new(move |event: Event| {
                let _ = sender.try_send(event.type_() == "change");
            });
            let picker = Self { input, completion };
            for event in ["change", "cancel"] {
                picker
                    .input
                    .add_event_listener_with_callback(
                        event,
                        picker.completion.as_ref().unchecked_ref(),
                    )
                    .map_err(|_| "Could not listen for image selection.")?;
            }
            document
                .body()
                .ok_or("The browser document is unavailable.")?
                .append_child(&picker.input)
                .map_err(|_| "Could not open the image picker.")?;
            picker
                .input
                .show_picker()
                .map_err(|_| "Could not open the image picker. Try attaching again.")?;
            Ok((picker, receiver))
        }

        fn files(&self) -> Result<Vec<File>, String> {
            let files = self.input.files().ok_or("Could not read the selection.")?;
            if files.length() as usize > MAX_AGENT_PROMPT_IMAGES {
                return Err(format!(
                    "Attach at most {MAX_AGENT_PROMPT_IMAGES} images per message."
                ));
            }
            let mut selected = Vec::with_capacity(files.length() as usize);
            let mut bytes = 0.0;
            for index in 0..files.length() {
                let file = files.get(index).ok_or("Could not read the selection.")?;
                super::validate_format(&file.type_())?;
                bytes += file.size();
                if !bytes.is_finite() || bytes > MAX_AGENT_PROMPT_BYTES as f64 {
                    return Err(super::size_error());
                }
                selected.push(file);
            }
            Ok(selected)
        }
    }

    impl Drop for Picker {
        fn drop(&mut self) {
            for event in ["change", "cancel"] {
                let _ = self.input.remove_event_listener_with_callback(
                    event,
                    self.completion.as_ref().unchecked_ref(),
                );
            }
            self.input.remove();
        }
    }

    pub async fn choose_images() -> Result<Vec<AgentImage>, String> {
        let (picker, receiver) = Picker::open()?;
        if !receiver.recv().await.unwrap_or(false) {
            return Ok(Vec::new());
        }
        let files = picker.files()?;
        drop(picker);
        let mut images = Vec::with_capacity(files.len());
        for file in files {
            let buffer = JsFuture::from(file.array_buffer())
                .await
                .map_err(|_| "Could not read the selected image.")?;
            images.push(AgentImage {
                format: file.type_(),
                data: js_sys::Uint8Array::new(&buffer).to_vec(),
            });
        }
        super::validate_images("", &images)?;
        Ok(images)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(format: &str, bytes: usize) -> AgentImage {
        AgentImage {
            format: format.into(),
            data: vec![0; bytes],
        }
    }

    #[test]
    fn accepts_supported_images_and_empty_selection() {
        assert!(validate_images("Hello", &[]).is_ok());
        for format in ["image/png", "image/jpeg", "image/webp"] {
            assert!(validate_images("Hello", &[image(format, 1)]).is_ok());
        }
    }

    #[test]
    fn rejects_unsupported_and_empty_images() {
        assert!(validate_images("", &[image("image/svg+xml", 1)]).is_err());
        assert!(validate_images("", &[image("image/png", 0)]).is_err());
    }

    #[test]
    fn enforces_combined_prompt_bytes() {
        let images = [image("image/png", MAX_AGENT_PROMPT_BYTES - 2)];
        assert!(validate_images("é", &images).is_ok());
        assert!(validate_images("é!", &images).is_err());
        assert!(validate_images(&"a".repeat(MAX_AGENT_PROMPT_BYTES + 1), &[]).is_err());
        let images = [
            image("image/png", MAX_AGENT_PROMPT_BYTES / 2),
            image("image/jpeg", MAX_AGENT_PROMPT_BYTES / 2 + 1),
        ];
        assert!(validate_images("", &images).is_err());
    }

    #[test]
    fn enforces_protocol_image_count() {
        let mut images = vec![image("image/png", 1); MAX_AGENT_PROMPT_IMAGES];
        assert!(validate_images("", &images).is_ok());
        images.push(image("image/png", 1));
        assert!(validate_images("", &images).is_err());
    }
}
