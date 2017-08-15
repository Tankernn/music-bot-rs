extern crate discord;
extern crate toml;
extern crate serde;

#[macro_use]
extern crate serde_derive;

use std::fs::File;
use std::io::prelude::*;
use discord::{Discord, State};
use discord::model::{Event, ChannelId};

#[derive(Debug, Deserialize)]
struct Config {
    discord_token: String,
    command_prefix: String,
    command_channel: Option<u64>,
}

pub fn main() {
    // Load config file
    let mut file = File::open("config/config.toml").expect("failed to open config file");
    let mut contents = String::new();
    file.read_to_string(&mut contents).expect("failed to read config file");
    let config: Config = toml::from_str(&contents).expect("failed to parse config file");

    // Log in to Discord using a bot token from the environment
    let discord = Discord::from_bot_token(&config.discord_token).expect("login failed");

    // establish websocket and voice connection
    let (mut connection, ready) = discord.connect().expect("connect failed");
    println!("[Ready] {} is serving {} servers", ready.user.username, ready.servers.len());
    let mut state = State::new(ready);
    connection.sync_calls(&state.all_private_channels());

    // receive events forever
    loop {
        let event = match connection.recv_event() {
            Ok(event) => event,
            Err(err) => {
                println!("[Warning] Receive error: {:?}", err);
                if let discord::Error::WebSocket(..) = err {
                    // Handle the websocket connection being dropped
                    let (new_connection, ready) = discord.connect().expect("connect failed");
                    connection = new_connection;
                    state = State::new(ready);
                    println!("[Ready] Reconnected successfully.");
                }
                if let discord::Error::Closed(..) = err {
                    break
                }
                continue
            },
        };
        state.update(&event);

        match event {
            Event::MessageCreate(message) => {
                // safeguard: stop if the message is from us
                if message.author.id == state.user().id {
                    continue
                }

                // ignore message outside of command channel
                if let Some(channel_id) = config.command_channel {
                    if ChannelId(channel_id) != message.channel_id {
                        continue
                    }
                }

                // reply to a command if there was one
                let mut split = message.content.split_whitespace();
                let first_word = split.next().unwrap_or("");
                let argument = split.next().unwrap_or("");

                let prefix = &config.command_prefix;

                if first_word.starts_with(prefix) {
                    let vchan = state.find_voice_user(message.author.id);
                    let command: String = first_word.chars().skip(prefix.chars().count()).collect();

                    match command.as_ref() {
                        "stop" => {
                            vchan.map(|(sid, _)| connection.voice(sid).stop());
                        },
                        "quit" => {
                            vchan.map(|(sid, _)| connection.drop_voice(sid));
                        },
                        "play" => {
                            let output = if let Some((server_id, channel_id)) = vchan {
                                match discord::voice::open_ytdl_stream(argument) {
                                    Ok(stream) => {
                                        let voice = connection.voice(server_id);
                                        voice.set_deaf(true);
                                        voice.connect(channel_id);
                                        voice.play(stream);
                                        String::new()
                                    },
                                    Err(error) => format!("Error: {}", error),
                                }
                            } else {
                                "You must be in a voice channel to DJ".to_owned()
                            };
                            if !output.is_empty() {
                                warn(discord.send_message(message.channel_id, &output, "", false));
                            }
                        },
                        _ => {

                        }
                    }
                }
            }
            Event::VoiceStateUpdate(server_id, _) => {
                // If someone moves/hangs up, and we are in a voice channel,
                if let Some(cur_channel) = connection.voice(server_id).current_channel() {
                    // and our current voice channel is empty, disconnect from voice
                    match server_id {
                        Some(server_id) => if let Some(srv) = state.servers().iter().find(|srv| srv.id == server_id) {
                            if srv.voice_states.iter().filter(|vs| vs.channel_id == Some(cur_channel)).count() <= 1 {
                                connection.voice(Some(server_id)).disconnect();
                            }
                        },
                        None => if let Some(call) = state.calls().get(&cur_channel) {
                            if call.voice_states.len() <= 1 {
                                connection.voice(server_id).disconnect();
                            }
                        }
                    }
                }
            }
            _ => {}, // discard other events
        }
    }
}

fn warn<T, E: ::std::fmt::Debug>(result: Result<T, E>) {
    match result {
        Ok(_) => {},
        Err(err) => println!("[Warning] {:?}", err)
    }
}
