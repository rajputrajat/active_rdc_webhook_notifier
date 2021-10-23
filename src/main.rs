use anyhow::{anyhow, Result};
use clap::{App, Arg};
use log::info;
use rdc_connections::RemoteServer;
use simple_webhook_msg_sender::WebhookSender;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let input = process_cmd_args()?;
    let msg_sender = WebhookSender::new(&input.url);
    let mut server_handlers = Vec::new();
    for server_name in input.servers {
        server_handlers.push(RemoteServer::new(&server_name)?);
    }
    Ok(())
}

fn process_cmd_args() -> Result<UserInput> {
    let mut m = App::new("Active RDC Webhook notifier")
        .author("Rajat Rajput <rajputrajat@gmail.com>")
        .arg(
            Arg::with_name("server")
                .long("server")
                .value_name("windows server name")
                .multiple(true)
                .required(true),
        )
        .arg(
            Arg::with_name("webhook url")
                .long("url")
                .value_name("webhook url")
                .last(true)
                .required(true),
        )
        .get_matches();
    let servers: Vec<String> = m
        .values_of("server")
        .ok_or_else(|| anyhow!("'server' input is missing"))?
        .into_iter()
        .map(|s| s.to_owned())
        .collect();
    let url = m
        .value_of("webhook url")
        .ok_or_else(|| anyhow!("'webhook url' input is missing"))?
        .to_owned();
    Ok(UserInput { servers, url })
}

struct UserInput {
    servers: Vec<String>,
    url: String,
}
