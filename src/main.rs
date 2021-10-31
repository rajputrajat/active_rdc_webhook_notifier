use anyhow::{anyhow, Result};
use chrono::Local;
use clap::{App, Arg};
use env_logger::Builder;
use log::{error, info};
use rdc_connections::{RemoteDesktopSessionInfo, RemoteDesktopSessionState, RemoteServer};
use simple_webhook_msg_sender::WebhookSender;
use std::{
    collections::{hash_map::Entry, HashMap},
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::time::{sleep, Duration};

type MsgSender = Arc<WebhookSender>;
type ServerClientMapShared = Arc<Mutex<ServerClientMap>>;
type ServerClientMap = HashMap<String, ClientStateMap>;

#[derive(Debug)]
struct ClientStateMap {
    data: HashMap<String, ClientData>,
}

#[derive(Debug)]
struct ClientData {
    state: RemoteDesktopSessionState,
    user: String,
}

impl ClientStateMap {
    fn update_state(&mut self, client_info: &[RemoteDesktopSessionInfo]) -> Vec<String> {
        const ACTIVATED: &str = "is now connected to";
        const DEACTIVATED: &str = "is disconnected from";
        let mut return_value: Vec<String> = Vec::new();
        client_info.iter().for_each(|i| {
            let client = &i.client_info.client;
            let user = &i.client_info.user;
            let current_state = &i.state;
            if let Entry::Vacant(e) = self.data.entry(client.to_owned()) {
                e.insert(ClientData {
                    state: *current_state,
                    user: user.to_owned(),
                });
                if current_state == &RemoteDesktopSessionState::Active {
                    return_value.push(format!("'{}' {}", client, ACTIVATED));
                }
            } else {
                let prev_state = self.data.get_mut(client).unwrap();
                if current_state == &RemoteDesktopSessionState::Active {
                    if prev_state.state != RemoteDesktopSessionState::Active {
                        return_value.push(format!("'{}' {}", client, ACTIVATED));
                    }
                } else if current_state != &RemoteDesktopSessionState::Active
                    && prev_state.state == RemoteDesktopSessionState::Active
                {
                    return_value.push(format!("'{}' {}", client, DEACTIVATED));
                }
                *prev_state = ClientData {
                    state: *current_state,
                    user: user.to_owned(),
                };
            }
        });
        // in case client is not found
        for client in &mut self.data {
            if !client_info
                .iter()
                .any(|i| &i.client_info.client == client.0)
                && (client.1.state == RemoteDesktopSessionState::Active)
            {
                client.1.state = RemoteDesktopSessionState::Disconnected;
                return_value.push(format!("'{}' {}", client.0, DEACTIVATED));
            }
        }
        return_value
    }
}

#[tokio::main]
async fn main() -> ! {
    initilize_logger();
    let input = process_cmd_args().unwrap();
    let msg_sender = Arc::new(WebhookSender::new(&input.url));
    let state_map: ServerClientMapShared = Arc::new(Mutex::new(HashMap::new()));
    for server in &input.servers {
        state_map.lock().unwrap().insert(
            server.clone(),
            ClientStateMap {
                data: HashMap::new(),
            },
        );
    }
    loop {
        match refresh_all_connections(msg_sender.clone(), input.servers.clone(), state_map.clone())
            .await
        {
            Ok(_) => {}
            Err(e) => error!("{:?}", e),
        }
        info!("{:?}", state_map);
        sleep(input.period).await;
    }
}

fn initilize_logger() {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
}

async fn refresh_all_connections(
    msg_sender: MsgSender,
    servers: Vec<String>,
    state_map: ServerClientMapShared,
) -> Result<()> {
    let mut tasks = Vec::new();
    for server in servers {
        let state_map = state_map.clone();
        tasks.push(tokio::task::spawn(async move {
            match RemoteServer::new(server) {
                Ok(handler) => read_active_connections(handler, state_map),
                Err(e) => {
                    error!("{:?}", e);
                    Vec::new()
                }
            }
        }));
    }
    for t in tasks {
        //let connection_status = t.await??;
        match t.await {
            Ok(connection_status) => {
                info!("messages: {:?}", connection_status);
                for st in &connection_status {
                    msg_sender.post(st).await?;
                }
            }
            Err(e) => error!("{:?}", e),
        }
    }
    Ok(())
}

fn read_active_connections(
    mut server_handle: RemoteServer,
    state_map: ServerClientMapShared,
) -> Vec<String> {
    let mut connection_info = Vec::new();
    match server_handle.get_updated_info() {
        Ok(server_info_v) => {
            info!("{:?}", server_info_v);
            let mut locked_state = state_map.lock().unwrap();
            let client_state_map = locked_state.get_mut(&server_handle.name).unwrap(); // unwrap is fine here
            let conn_status_vec = client_state_map.update_state(&server_info_v);
            conn_status_vec.iter().for_each(|out_string| {
                connection_info.push(format!("{} '{}'", out_string, &server_handle.name));
            });
        }
        Err(e) => error!("{:?}", e),
    }
    connection_info
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
                .required(true)
                .multiple(false),
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
