use std::{env, fs, path::PathBuf};

use zz_gpui_kit::icon::glyphs;

fn main() {
    let usage = "usage: export_icons <dir> <theme radius> <corner smoothing>";
    let mut args = env::args().skip(1);
    let dir = PathBuf::from(args.next().expect(usage));
    let radius: f32 = args.next().expect(usage).parse().expect(usage);
    let smoothing: f32 = args.next().expect(usage).parse().expect(usage);
    fs::create_dir_all(&dir).expect("the output directory is writable");
    for name in glyphs::names() {
        let svg = glyphs::svg(name, radius, smoothing).expect("every icon in the set draws");
        fs::write(dir.join(format!("{name}.svg")), svg + "\n").expect("the icon is writable");
    }
    println!("{} icons -> {}", glyphs::names().count(), dir.display());
}
