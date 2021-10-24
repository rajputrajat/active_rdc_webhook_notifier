use anyhow::{anyhow, Result};
use clap::{App, Arg};
use log::info;
use rdc_connections::{RemoteDesktopSessionState, RemoteServer};
use simple_webhook_msg_sender::WebhookSender;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};
use tokio::time::{sleep, Duration};

type MsgSender = Arc<WebhookSender>;
type StateMapShared = Arc<ServerClientMap>;

struct ClientStateMap {
    data: HashMap<String, RemoteDesktopSessionState>,
}

struct ServerClientMap {
    data: HashMap<String, ClientStateMap>,
}

impl ServerClientMap {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    fn register_server(&mut self, server: &str) {
        self.data.insert(server.to_owned(), ClientStateMap::new());
    }

    fn update_state(
        &mut self,
        server: &str,
        client: &str,
        current_state: RemoteDesktopSessionState,
    ) -> Result<String> {
    }
}

impl ClientStateMap {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    fn update_state(
        &mut self,
        client: &str,
        current_state: RemoteDesktopSessionState,
    ) -> Result<String> {
    }
}

#[tokio::main]
async fn main() -> ! {
    env_logger::init();
    let input = process_cmd_args().unwrap();
    let msg_sender = Arc::new(WebhookSender::new(&input.url));
    let client_state_map: ServerClientMap = Arc::new(ClientStateMap::new());
    loop {
        refresh_all_connections(
            msg_sender.clone(),
            input.servers.clone(),
            client_state_map.clone(),
        )
        .await
        .unwrap();
        sleep(input.period).await;
    }
}

async fn refresh_all_connections(
    msg_sender: MsgSender,
    servers: Vec<String>,
    client_state_map: ClientStateMapShared,
) -> Result<()> {
    let mut tasks = Vec::new();
    for server in servers {
        tasks.push(tokio::task::spawn(async move {
            let handler = RemoteServer::new(server)?;
            read_active_connections(handler)
        }));
    }
    for t in tasks {
        let connection_status = t.await??;
        msg_sender.post(&connection_status).await?;
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
        .arg(
            Arg::with_name("period")
                .long("period")
                .value_name("period between")
                .multiple(false)
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
    let period = {
        let p_str = m
            .value_of("period")
            .ok_or_else(|| anyhow!("'period' is mandatory"))?;
        Duration::from_secs(p_str.parse::<u64>()?)
    };
    Ok(UserInput {
        servers,
        url,
        period,
    })
}

struct UserInput {
    servers: Vec<String>,
    url: String,
    period: Duration,
}
