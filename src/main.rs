use anyhow::{anyhow, Result};
use clap::{App, Arg};
use log::info;
use rdc_connections::{RemoteDesktopSessionState, RemoteServer};
use simple_webhook_msg_sender::WebhookSender;
use std::sync::{Arc, Mutex};

type MsgSender = Arc<WebhookSender>;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let input = process_cmd_args()?;
    let msg_sender = Arc::new(WebhookSender::new(&input.url));
    refresh_all_connections(msg_sender, input.servers).await?;
    Ok(())
}

async fn refresh_all_connections(msg_sender: MsgSender, servers: Vec<String>) -> Result<()> {
    let mut tasks = Vec::new();
    for server in servers {
        tasks.push(tokio::task::spawn(async move {
            let handler = RemoteServer::new(server)?;
            read_active_connections(handler)
        }));
    }
    for t in tasks {
        let connection_status = t.await?;
    }
    Ok(())
}

fn read_active_connections(mut server_handle: RemoteServer) -> Result<String> {
    let server_info_v = server_handle.get_updated_info()?;
    let mut connection_info = String::new();
    server_info_v
        .iter()
        .filter(|&s| s.state == RemoteDesktopSessionState::Active)
        .for_each(|i| {
            connection_info.push_str(&format!(
                "Connected user: {:?}, Client: {:?}, from address: {:?}",
                i.client_info.user, i.client_info.client, i.client_info.address
            ));
        });
    Ok(connection_info)
}

fn process_cmd_args() -> Result<UserInput> {
    let m = App::new("Active RDC Webhook notifier")
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
