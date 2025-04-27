use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet};
use std::thread;
use std::fs;
use std::io::{Write, Read};
use tungstenite::accept;
use tungstenite::protocol::Message;
use serde_json::{json, Value};

#[derive(Clone)]
struct Client {
    sender: Arc<Mutex<tungstenite::protocol::WebSocket<std::net::TcpStream>>>,
    id: usize,
    voice_room: Option<String>,
}

struct VoiceRoom {
    participants: HashSet<usize>,
}

struct AppState {
    clients: HashMap<usize, Client>,
    voice_rooms: HashMap<String, VoiceRoom>,
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    let state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState {
        clients: HashMap::new(),
        voice_rooms: HashMap::new(),
    }));
    let mut client_id_counter = 0;

    println!("Server running on ws://127.0.0.1:8080");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                let client_id = client_id_counter;
                client_id_counter += 1;

                thread::spawn(move || {
                    let mut header = [0; 4];
                    if stream.peek(&mut header).is_ok() {
                        if header.starts_with(b"GET ") {
                            handle_http_request(stream);
                            return;
                        }
                    }

                    match accept(stream) {
                        Ok(websocket) => {
                            println!("New WebSocket client connected: {}", client_id);
                            let client = Client {
                                sender: Arc::new(Mutex::new(websocket)),
                                id: client_id,
                                voice_room: None,
                            };

                            {
                                let mut state = state.lock().unwrap();
                                state.clients.insert(client_id, client.clone());
                                broadcast_user_list(&mut state.clients);
                            }
                            
                            handle_client(client, state);
                        }
                        Err(e) => println!("WebSocket handshake failed: {}", e),
                    }
                });
            }
            Err(e) => println!("Connection failed: {}", e),
        }
    }
}

fn handle_http_request(mut stream: std::net::TcpStream) {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer).unwrap();
    let request = String::from_utf8_lossy(&buffer);
    
    let path = request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
    let file_path = if path == "/" {
        "static/index.html"
    } else {
        &path[1..]
    };

    match fs::read(file_path) {
        Ok(contents) => {
            let content_type = match file_path.split('.').last().unwrap() {
                "html" => "text/html",
                "js" => "application/javascript",
                "css" => "text/css",
                _ => "application/octet-stream",
            };

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
                content_type,
                contents.len()
            );

            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&contents).unwrap();
        }
        Err(_) => {
            let response = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
            stream.write_all(response.as_bytes()).unwrap();
        }
    }
}

fn handle_client(client: Client, state: Arc<Mutex<AppState>>) {
    loop {
        let msg = match client.sender.lock().unwrap().read() {
            Ok(Message::Text(msg)) => msg,
            Ok(Message::Close(_)) | Err(_) => {
                handle_client_disconnect(&client, &state);
                break;
            }
            _ => continue,
        };

        let parsed: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => continue,
        };
        
        match parsed["type"].as_str().unwrap_or("") {
            "chat" => {
                let broadcast_msg = json!({
                    "type": "chat",
                    "userId": client.id,
                    "message": parsed["message"]
                }).to_string();

                let mut state = state.lock().unwrap();
                broadcast_message_to_all(&mut state.clients, &broadcast_msg, Some(client.id));
            }
            "join-voice" => {
                if let Some(room_id) = parsed["roomId"].as_str() {
                    handle_join_voice_room(&client, room_id, &state);
                }
            }
            "leave-voice" => {
                if let Some(room_id) = parsed["roomId"].as_str() {
                    handle_leave_voice_room(&client, room_id, &state);
                }
            }
            "voice-invite" => {
                if let Some(target_id) = parsed["targetUserId"].as_u64() {
                    let room_id = parsed["roomId"].as_str().unwrap_or("");
                    let mut state = state.lock().unwrap();
                    if let Some(target) = state.clients.get_mut(&(target_id as usize)) {
                        let invite_msg = json!({
                            "type": "voice-invite",
                            "userId": client.id,
                            "roomId": room_id
                        }).to_string();
                        let _ = target.sender.lock().unwrap().send(Message::Text(invite_msg));
                    }
                }
            }
            "offer" | "answer" | "ice-candidate" => {
                if let Some(target_id) = parsed["targetUserId"].as_u64() {
                    let mut state = state.lock().unwrap();
                    if let Some(target) = state.clients.get_mut(&(target_id as usize)) {
                        let _ = target.sender.lock().unwrap().send(Message::Text(msg));
                    }
                }
            }
            _ => {}
        }
    }
}

fn handle_client_disconnect(client: &Client, state: &Arc<Mutex<AppState>>) {
    let mut state = state.lock().unwrap();
    if let Some(room_id) = &client.voice_room {
        if let Some(room) = state.voice_rooms.get_mut(room_id) {
            room.participants.remove(&client.id);
            if room.participants.is_empty() {
                state.voice_rooms.remove(room_id);
            }
        }
    }
    state.clients.remove(&client.id);
    broadcast_user_list(&mut state.clients);
}

fn handle_join_voice_room(client: &Client, room_id: &str, state: &Arc<Mutex<AppState>>) {
    let mut state = state.lock().unwrap();
    let room = state.voice_rooms.entry(room_id.to_string())
        .or_insert_with(|| VoiceRoom {
            participants: HashSet::new(),
        });

    room.participants.insert(client.id);
    if let Some(client) = state.clients.get_mut(&client.id) {
        client.voice_room = Some(room_id.to_string());
    }

    let join_msg = json!({
        "type": "join-voice",
        "userId": client.id,
        "roomId": room_id
    }).to_string();

    broadcast_to_room(&mut state.clients, room_id, &join_msg, Some(client.id));
}

fn handle_leave_voice_room(client: &Client, room_id: &str, state: &Arc<Mutex<AppState>>) {
    let mut state = state.lock().unwrap();
    if let Some(room) = state.voice_rooms.get_mut(room_id) {
        room.participants.remove(&client.id);
        if room.participants.is_empty() {
            state.voice_rooms.remove(room_id);
        }
    }

    if let Some(client) = state.clients.get_mut(&client.id) {
        client.voice_room = None;
    }

    let leave_msg = json!({
        "type": "leave-voice",
        "userId": client.id,
        "roomId": room_id
    }).to_string();

    broadcast_to_room(&mut state.clients, room_id, &leave_msg, None);
}

fn broadcast_message_to_all(clients: &mut HashMap<usize, Client>, message: &str, exclude_id: Option<usize>) {
    for (id, client) in clients.iter_mut() {
        if Some(*id) != exclude_id {
            let _ = client.sender.lock().unwrap().send(Message::Text(message.to_string()));
        }
    }
}

fn broadcast_to_room(clients: &mut HashMap<usize, Client>, room_id: &str, message: &str, exclude_id: Option<usize>) {
    for (id, client) in clients.iter_mut() {
        if Some(*id) != exclude_id && client.voice_room.as_deref() == Some(room_id) {
            let _ = client.sender.lock().unwrap().send(Message::Text(message.to_string()));
        }
    }
}

fn broadcast_user_list(clients: &mut HashMap<usize, Client>) {
    let user_list: Vec<usize> = clients.keys().cloned().collect();
    let message = json!({
        "type": "userList",
        "users": user_list
    }).to_string();

    for client in clients.values_mut() {
        let _ = client.sender.lock().unwrap().send(Message::Text(message.clone()));
    }
}
