use anyhow::{anyhow, Result};
use clap::{App, Arg};
use log::info;
use rdc_connections::{RemoteDesktopSessionState, RemoteServer};
use simple_webhook_msg_sender::WebhookSender;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::{Arc, Mutex},
};
use tokio::time::{sleep, Duration};

type MsgSender = Arc<WebhookSender>;
type ServerClientMapShared = Arc<Mutex<ServerClientMap>>;
type ServerClientMap = HashMap<String, ClientStateMap>;

struct ClientStateMap {
    data: HashMap<String, RemoteDesktopSessionState>,
}

impl ClientStateMap {
    fn update_state(
        &mut self,
        client: &str,
        current_state: &RemoteDesktopSessionState,
    ) -> Option<String> {
        const ACTIVATED: &str = "is now connected to";
        const DEACTIVATED: &str = "is disconnected from";
        let mut return_value: Option<String> = None;
        if let Entry::Vacant(e) = self.data.entry(client.to_owned()) {
            e.insert(*current_state);
            return_value = Some(format!("'{}' {}", client, ACTIVATED));
        } else {
            let prev_state = self.data.get_mut(client).unwrap();
            if current_state == &RemoteDesktopSessionState::Active {
                if prev_state != &RemoteDesktopSessionState::Active {
                    return_value = Some(format!("'{}' {}", client, ACTIVATED));
                } else {
                    return_value = Some(format!("'{}' {}", client, DEACTIVATED));
                }
            }
            *prev_state = *current_state;
        }
        return_value
    }
}

#[tokio::main]
async fn main() -> ! {
    env_logger::init();
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
        refresh_all_connections(msg_sender.clone(), input.servers.clone(), state_map.clone())
            .await
            .unwrap();
        sleep(input.period).await;
    }
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
            let handler = RemoteServer::new(server)?;
            read_active_connections(handler, state_map)
        }));
    }
    for t in tasks {
        let connection_status = t.await??;
        info!("messages: {}", connection_status);
        msg_sender.post(&connection_status).await?;
    }
    Ok(())
}

fn read_active_connections(
    mut server_handle: RemoteServer,
    state_map: ServerClientMapShared,
) -> Result<String> {
    let server_info_v = server_handle.get_updated_info()?;
    let mut connection_info = String::new();
    server_info_v.iter().for_each(|i| {
        let mut locked_state = state_map.lock().unwrap();
        let client_state_map = locked_state.get_mut(&server_handle.name).unwrap(); // unwrap is fine here
        if let Some(out_string) = client_state_map.update_state(&i.client_info.client, &i.state) {
            connection_info.push_str(&format!("{} '{}'\n", out_string, &server_handle.name));
        }
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
