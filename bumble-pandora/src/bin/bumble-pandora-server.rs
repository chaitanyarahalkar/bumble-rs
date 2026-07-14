use bumble_pandora::config::{DEFAULT_GRPC_PORT, DEFAULT_ROOTCANAL_PORT};
use bumble_pandora::proto::host_server::HostServer;
use bumble_pandora::proto::l2cap::l2cap_server::L2capServer;
use bumble_pandora::{HostService, L2capService, PandoraConfig, PandoraRuntime};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use tonic::transport::Server;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    grpc_port: u16,
    rootcanal_port: u16,
    transport: Option<String>,
    config: Option<PathBuf>,
}

fn usage() -> &'static str {
    "usage: bumble-pandora-server [--grpc-port PORT] [--rootcanal-port PORT] [--transport TRANSPORT] [--config FILE]"
}

fn option_value(
    argument: &str,
    option: &str,
    values: &[String],
    index: &mut usize,
) -> Result<Option<String>, String> {
    if argument == option {
        *index += 1;
        return values
            .get(*index)
            .cloned()
            .map(Some)
            .ok_or_else(|| format!("missing value for {option}"));
    }
    Ok(argument
        .strip_prefix(&format!("{option}="))
        .map(ToOwned::to_owned))
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Option<Args>, String> {
    let values = arguments.into_iter().skip(1).collect::<Vec<_>>();
    let mut args = Args {
        grpc_port: DEFAULT_GRPC_PORT,
        rootcanal_port: DEFAULT_ROOTCANAL_PORT,
        transport: None,
        config: None,
    };
    let mut index = 0;
    while index < values.len() {
        let argument = values[index].as_str();
        if matches!(argument, "-h" | "--help") {
            return Ok(None);
        }
        if let Some(value) = option_value(argument, "--grpc-port", &values, &mut index)? {
            args.grpc_port = value
                .parse()
                .map_err(|_| format!("invalid gRPC port {value:?}"))?;
        } else if let Some(value) = option_value(argument, "--rootcanal-port", &values, &mut index)?
        {
            args.rootcanal_port = value
                .parse()
                .map_err(|_| format!("invalid RootCanal port {value:?}"))?;
        } else if let Some(value) = option_value(argument, "--transport", &values, &mut index)? {
            args.transport = Some(value);
        } else if let Some(value) = option_value(argument, "--config", &values, &mut index)? {
            args.config = Some(PathBuf::from(value));
        } else {
            return Err(format!("unknown argument {argument:?}"));
        }
        index += 1;
    }
    Ok(Some(args))
}

async fn run(args: Args) -> Result<(), String> {
    let config = match args.config.as_deref() {
        Some(path) => PandoraConfig::from_json_file(path)?,
        None => PandoraConfig::default(),
    };
    let runtime = PandoraRuntime::open(config, args.rootcanal_port, args.transport.as_deref())?;
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), args.grpc_port);
    println!("Pandora gRPC server listening on {address}");
    Server::builder()
        .add_service(HostServer::new(HostService::new(runtime.clone())))
        .add_service(L2capServer::new(L2capService::new(runtime)))
        .serve_with_shutdown(address, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    match parse_args(std::env::args()) {
        Ok(None) => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Ok(Some(args)) => {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("failed to create Tokio runtime: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(run(args)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(values: &[&str]) -> Result<Option<Args>, String> {
        parse_args(values.iter().map(ToString::to_string))
    }

    #[test]
    fn cli_matches_upstream_defaults_and_options() {
        assert_eq!(
            parsed(&["pandora"]).unwrap(),
            Some(Args {
                grpc_port: 7999,
                rootcanal_port: 7300,
                transport: None,
                config: None,
            })
        );
        assert_eq!(
            parsed(&[
                "pandora",
                "--grpc-port=8000",
                "--rootcanal-port",
                "6402",
                "--transport",
                "tcp-client:localhost:1234",
                "--config=device.json",
            ])
            .unwrap(),
            Some(Args {
                grpc_port: 8000,
                rootcanal_port: 6402,
                transport: Some("tcp-client:localhost:1234".into()),
                config: Some(PathBuf::from("device.json")),
            })
        );
        assert!(parsed(&["pandora", "--wat"]).is_err());
        assert_eq!(parsed(&["pandora", "--help"]).unwrap(), None);
    }
}
