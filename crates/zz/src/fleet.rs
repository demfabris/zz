use zz_daemon::Endpoint;

use crate::config::{self, RejectedHost};

const FLEET_USAGE: &str = "usage: zz fleet add <name> <ssh-destination>\n       zz fleet list [-F <format>]\n       zz fleet remove <name>";
const FLEET_ADD_USAGE: &str = "usage: zz fleet add <name> <ssh-destination>";
const FLEET_LIST_USAGE: &str = "usage: zz fleet list [-F <format>]";
const FLEET_REMOVE_USAGE: &str = "usage: zz fleet remove <name>";

pub(crate) fn run(arguments: impl IntoIterator<Item = String>) -> Result<String, String> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let Some((command, arguments)) = arguments.split_first() else {
        return Err(FLEET_USAGE.to_owned());
    };
    match command.as_str() {
        "add" => fleet_add(&parse_fleet_add(arguments)?),
        "list" => fleet_list(parse_fleet_list(arguments)?),
        "remove" => {
            let [name] = arguments else {
                return Err(FLEET_REMOVE_USAGE.to_owned());
            };
            Ok(format_forget_outcome(&forget_host(name)?))
        }
        _ => Err(FLEET_USAGE.to_owned()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ForgetOutcome {
    pub(crate) name: String,
    pub(crate) endpoint: String,
    pub(crate) config_removed: bool,
}

pub(crate) fn forget_host(name: &str) -> Result<ForgetOutcome, String> {
    let (hosts, rejected) = config::configured_fleet_hosts()
        .map_err(|error| format!("could not read zz/config: {error}"))?;
    let endpoint = hosts
        .iter()
        .find(|host| host.name == name)
        .map(|host| host.endpoint.to_string())
        .or_else(|| {
            rejected
                .iter()
                .find(|host| host.name == name)
                .map(|host| host.value.clone())
        });
    let Some(endpoint) = endpoint else {
        let known = hosts
            .iter()
            .map(|host| host.name.as_str())
            .chain(rejected.iter().map(|host| host.name.as_str()))
            .collect::<Vec<_>>();
        let known = if known.is_empty() {
            "(none)".to_owned()
        } else {
            known.join(", ")
        };
        return Err(format!("unknown fleet host `{name}`; known hosts: {known}"));
    };

    let config_removed = config::remove_fleet_host(name)
        .map_err(|error| format!("could not remove host-{name} from zz/config: {error}"))?;

    Ok(ForgetOutcome {
        name: name.to_owned(),
        endpoint,
        config_removed,
    })
}

fn parse_fleet_list(arguments: &[String]) -> Result<Option<&str>, String> {
    match arguments {
        [] => Ok(None),
        [option, format] if option == "-F" => Ok(Some(format)),
        _ => Err(FLEET_LIST_USAGE.to_owned()),
    }
}

fn fleet_list(format: Option<&str>) -> Result<String, String> {
    let (hosts, rejected) = config::configured_fleet_hosts()
        .map_err(|error| format!("could not read zz/config: {error}"))?;
    let rows = hosts
        .into_iter()
        .map(|host| (host.name, host.endpoint.to_string()))
        .collect::<Vec<_>>();
    if let Some(format) = format {
        return Ok(format_fleet_rows_with_format(&rows, format));
    }
    Ok(format_fleet_listing(&rows, &rejected))
}

fn format_fleet_rows_with_format(rows: &[(String, String)], format: &str) -> String {
    rows.iter()
        .map(|(name, endpoint)| {
            format
                .replace("#{host_name}", name)
                .replace("#{host_endpoint}", endpoint)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_fleet_listing(rows: &[(String, String)], rejected: &[RejectedHost]) -> String {
    let listing = format_fleet_rows(rows);
    if rejected.is_empty() {
        return listing;
    }
    let dropped = std::iter::once("dropped (`zz fleet remove <name>` deletes these):".to_owned())
        .chain(
            rejected
                .iter()
                .map(|host| format!("  host-{}: {}", host.name, host.reason)),
        )
        .collect::<Vec<_>>()
        .join("\n");
    if listing.is_empty() {
        dropped
    } else {
        format!("{listing}\n\n{dropped}")
    }
}

fn format_fleet_rows(rows: &[(String, String)]) -> String {
    let name_width = rows
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|(name, endpoint)| format!("{name:<name_width$}  {endpoint}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_forget_outcome(outcome: &ForgetOutcome) -> String {
    format!(
        "forgot {} ({})\nconfig line removed: {}",
        outcome.name,
        outcome.endpoint,
        yes_no(outcome.config_removed),
    )
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FleetAddArguments {
    name: String,
    endpoint: String,
}

fn parse_fleet_add(arguments: &[String]) -> Result<FleetAddArguments, String> {
    let [name, destination] = arguments else {
        return Err(FLEET_ADD_USAGE.to_owned());
    };
    let destination = destination
        .strip_prefix("ssh://")
        .unwrap_or(destination.as_str());
    if destination.starts_with('-') {
        return Err("ssh destination must not start with `-`".to_owned());
    }
    let endpoint = format!("ssh://{destination}");
    Endpoint::parse(&endpoint).map_err(|error| error.to_string())?;
    config::validate_fleet_host(name, &endpoint)?;
    Ok(FleetAddArguments {
        name: name.clone(),
        endpoint,
    })
}

fn fleet_add(arguments: &FleetAddArguments) -> Result<String, String> {
    config::write_fleet_host(&arguments.name, &arguments.endpoint)
        .map_err(|error| format!("could not write zz/config: {error}"))?;
    Ok(format!("host-{} = {}", arguments.name, arguments.endpoint))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, process::Command};

    use super::*;

    #[test]
    fn list_rows_align_on_the_name_column() {
        assert_eq!(
            format_fleet_rows(&[
                ("gpu".to_owned(), "ssh://gpu-box".to_owned()),
                ("desktop".to_owned(), "ssh://desktop".to_owned()),
            ]),
            "gpu      ssh://gpu-box\ndesktop  ssh://desktop"
        );
    }

    #[test]
    fn formatted_list_rows_use_literal_host_substitution() {
        let rows = [
            ("gpu".to_owned(), "ssh://gpu-box".to_owned()),
            ("desktop".to_owned(), "ssh://desktop".to_owned()),
        ];
        assert_eq!(
            format_fleet_rows_with_format(&rows, "#{host_name}=#{host_endpoint} #{unknown}",),
            "gpu=ssh://gpu-box #{unknown}\ndesktop=ssh://desktop #{unknown}"
        );
        assert_eq!(
            parse_fleet_list(&["-F".to_owned(), "#{host_name}".to_owned()]),
            Ok(Some("#{host_name}"))
        );
        assert!(parse_fleet_list(&["-F".to_owned()]).is_err());
    }

    #[test]
    fn listing_names_the_lines_the_parser_dropped() {
        assert_eq!(
            format_fleet_listing(
                &[("desktop".to_owned(), "ssh://desktop".to_owned())],
                &[RejectedHost {
                    name: "gpu".to_owned(),
                    value: "quic://gpu:9922".to_owned(),
                    reason: "quic endpoints were removed; use ssh://".to_owned(),
                }],
            ),
            "desktop  ssh://desktop\n\ndropped (`zz fleet remove <name>` deletes these):\n  \
             host-gpu: quic endpoints were removed; use ssh://"
        );
        assert_eq!(
            format_fleet_listing(&[("desktop".to_owned(), "ssh://desktop".to_owned())], &[]),
            "desktop  ssh://desktop"
        );
    }

    #[test]
    fn add_arguments_become_an_ssh_endpoint_and_reject_bad_names() {
        assert_eq!(
            parse_fleet_add(&["desktop".to_owned(), "fabrico@arch-desktop".to_owned()]).unwrap(),
            FleetAddArguments {
                name: "desktop".to_owned(),
                endpoint: "ssh://fabrico@arch-desktop".to_owned(),
            }
        );
        assert_eq!(
            parse_fleet_add(&["gpu".to_owned(), "gpu-box:2222".to_owned()])
                .unwrap()
                .endpoint,
            "ssh://gpu-box:2222"
        );
        assert_eq!(
            parse_fleet_add(&["gpu".to_owned(), "ssh://gpu-box".to_owned()])
                .unwrap()
                .endpoint,
            "ssh://gpu-box"
        );

        assert_eq!(
            parse_fleet_add(&["local".to_owned(), "gpu-box".to_owned()]).unwrap_err(),
            "invalid `host-local`: host name `local` is reserved"
        );
        assert!(parse_fleet_add(&["gpu".to_owned(), "user@@gpu-box".to_owned()]).is_err());
        assert!(parse_fleet_add(&["gpu".to_owned(), "gpu-box:0".to_owned()]).is_err());
        assert!(parse_fleet_add(&["gpu".to_owned()]).is_err());
    }

    #[test]
    fn add_list_and_remove_round_trip_through_the_config_file() {
        const TEST_NAME: &str =
            "fleet::tests::add_list_and_remove_round_trip_through_the_config_file";
        if std::env::var("ZZ_FLEET_TEST_CHILD").as_deref() == Ok(TEST_NAME) {
            run_config_round_trip_test();
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(TEST_NAME)
            .arg("--nocapture")
            .env("ZZ_FLEET_TEST_CHILD", TEST_NAME)
            .env("ZZ_DATA_DIR", directory.path().join("data"))
            .env("XDG_CONFIG_HOME", directory.path().join("config"))
            .env("HOME", directory.path().join("home"))
            .status()
            .unwrap();
        assert!(status.success(), "isolated fleet add test failed");
    }

    fn run_config_round_trip_test() {
        let config_home =
            PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").expect("test config home"));
        let config_path = config_home.join("zz/config");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(
            &config_path,
            "# keep\nhost-desktop = ssh://old-desktop # keep too\nshow-fps = true\n",
        )
        .unwrap();

        for _ in 0..2 {
            let output = run([
                "add".to_owned(),
                "desktop".to_owned(),
                "fabrico@arch-desktop".to_owned(),
            ])
            .unwrap();
            assert_eq!(output, "host-desktop = ssh://fabrico@arch-desktop");
        }
        assert_eq!(
            fs::read_to_string(&config_path).unwrap(),
            "# keep\nhost-desktop = ssh://fabrico@arch-desktop # keep too\nshow-fps = true\n"
        );

        assert_eq!(
            run(["list".to_owned()]).unwrap(),
            "desktop  ssh://fabrico@arch-desktop"
        );
        assert_eq!(
            run(["remove".to_owned(), "missing".to_owned()]).unwrap_err(),
            "unknown fleet host `missing`; known hosts: desktop"
        );

        let removed = run(["remove".to_owned(), "desktop".to_owned()]).unwrap();
        assert!(removed.contains("config line removed: yes"));
        assert_eq!(
            fs::read_to_string(&config_path).unwrap(),
            "# keep\nshow-fps = true\n"
        );
        assert_eq!(run(["list".to_owned()]).unwrap(), "");

        assert_eq!(
            run(["remove".to_owned(), "desktop".to_owned()]).unwrap_err(),
            "unknown fleet host `desktop`; known hosts: (none)"
        );

        fs::write(
            &config_path,
            "# keep\nhost-gpu = quic://gpu:9922\nshow-fps = true\n",
        )
        .unwrap();
        assert_eq!(
            run(["list".to_owned()]).unwrap(),
            "dropped (`zz fleet remove <name>` deletes these):\n  host-gpu: invalid endpoint URI \
             `quic://gpu:9922`: quic endpoints were removed; use ssh://"
        );
        assert_eq!(
            run(["remove".to_owned(), "missing".to_owned()]).unwrap_err(),
            "unknown fleet host `missing`; known hosts: gpu"
        );
        assert_eq!(
            run(["remove".to_owned(), "gpu".to_owned()]).unwrap(),
            "forgot gpu (quic://gpu:9922)\nconfig line removed: yes"
        );
        assert_eq!(
            fs::read_to_string(&config_path).unwrap(),
            "# keep\nshow-fps = true\n"
        );
    }
}
