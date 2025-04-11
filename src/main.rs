// main.rs
#![windows_subsystem = "windows"]
use tray_item::{IconSource, TrayItem};
use std::process;
use rumqttc::{MqttOptions, Client, Event, Packet, QoS};
use serde::Deserialize;
use win_toast_notify::{WinToastNotify, CropCircle};
use std::env;
use std::fs::File;
use std::io::Read;
use serde_json;
use std::sync::{Mutex, Arc};
use std::collections::HashSet;
use std::time::{Instant, Duration};

#[derive(Deserialize)]
struct Config {
    mqtt_server: String,
    mqtt_port: u16,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
    mqtt_topic: String,
    cleaning_cycle: u64 // Cleaning cycle in seconds (e.g., 43200 for 12 hours)
}


#[derive(Deserialize)]
struct NotificationData {
    title: Option<String>,
    body_message: Option<String>,
    message_id: String, // Using a unique UUID as message_id
    logo: Option<String>
}

fn load_config() -> Config {
    let mut file = File::open("config/config.json").expect("Failed to open config.json");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Failed to read config.json");

    let config: Config = serde_json::from_str(&contents).expect("Failed to parse config.json");
    config
}

/// Function to start the system tray with a "Quit" option to exit the application.
fn start_tray() {
    let mut tray: TrayItem = TrayItem::new(
        "Notify Listener",
        IconSource::Resource("tray-icon"), // Tray icon loaded from resources
    )
    .unwrap();

    // Add a menu item to the system tray to quit the application
    tray.add_menu_item("Quit", move || {
        process::exit(0); // Exit the program
    })
    .unwrap();

    // Park the main thread to keep the tray icon active
    std::thread::park(); // This will keep the main thread alive
}


/// Function to set up the MQTT client, subscribe to the notification topic, and reconnect as needed.
fn start_mqtt_client(config: Config) {
    // Shared state to track the processed message IDs.
    let seen_messages: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Spawn a thread to clear the seen messages set based on the cleaning cycle.
    let seen_messages_clone = Arc::clone(&seen_messages);
    let cleaning_cycle = config.cleaning_cycle;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(cleaning_cycle));
            let mut seen_messages_lock = seen_messages_clone.lock().unwrap();
            seen_messages_lock.clear();
            println!("Seen messages set cleared after {} seconds.", cleaning_cycle);
        }
    });

    // Specify the inactivity timeout to reconnect
    let timeout_seconds: u64 = 300;
    // Outer loop: try connecting to the MQTT broker indefinitely.
    loop {
        println!("Attempting to connect to the MQTT broker...");
        let mut mqttoptions = MqttOptions::new("rumqtt-sync", &config.mqtt_server, config.mqtt_port);
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        if let (Some(user_name), Some(password)) = (config.mqtt_username.clone(), config.mqtt_password.clone()) {
            mqttoptions.set_credentials(user_name, password);
        }

        let (client, mut connection) = Client::new(mqttoptions, 10);
        if let Err(e) = client.subscribe(&config.mqtt_topic, QoS::AtMostOnce) {
            println!("Failed to subscribe to topic '{}': {:?}. Retrying...", config.mqtt_topic, e);
            std::thread::sleep(Duration::from_secs(5));
            continue; // Try to reconnect.
        }

        // Set a timeout period for inactivity, e.g., 60 seconds.
        let mut last_event_time = Instant::now();

        let reconnect_required = loop {
            // First, check if the elapsed time since the last event exceeds the timeout.
            if last_event_time.elapsed() > Duration::from_secs(timeout_seconds) {
                println!(
                    "No events received in {} seconds. Forcing reconnection.",
                    timeout_seconds
                );
                break true;
            }

            // Use next() to poll for an event.
            let maybe_event = connection.iter().next();
            if maybe_event.is_none() {
                println!("Connection iterator returned None. Forcing reconnection.");
                break true;
            }
            // When an event is received, update our timeout tracking.
            match maybe_event.unwrap() {
                Ok(event) => {
                    // Update last_event_time upon receiving any event.
                    last_event_time = Instant::now();

                    match event {
                        Event::Incoming(Packet::Publish(publish)) => {
                            if let Ok(payload_str) = String::from_utf8(publish.payload.to_vec()) {
                                match serde_json::from_str::<NotificationData>(&payload_str) {
                                    Ok(notification_data) => {
                                        let message_id = notification_data.message_id;
                                        let mut seen_messages_lock = seen_messages.lock().unwrap();
                                        if seen_messages_lock.contains(&message_id) {
                                            println!("Duplicate message received, ignoring (message_id: {}).", message_id);
                                            continue;
                                        }
                                        seen_messages_lock.insert(message_id);
                                        let title = notification_data.title.unwrap_or_else(|| "Notification".to_string());
                                        let body_message = notification_data.body_message.unwrap_or_else(|| "Details".to_string());
                                        let logo_path = notification_data.logo.clone();
                                        show_toast_notification(title, body_message, logo_path);
                                    },
                                    Err(e) => {
                                        println!("Failed to deserialize notification data: {}. Error: {:?}", payload_str, e);
                                    },
                                }
                            }
                        },
                        // If any other event occurs, simply print it for debugging.
                        other => {
                            println!("Event: {:?}", other);
                        }
                    }
                },
                Err(e) => {
                    println!("Error in event loop: {:?}. Will attempt to reconnect.", e);
                    break true;
                }
            }
        };

        if reconnect_required {
            println!("Reconnecting in 5 seconds...");
            std::thread::sleep(Duration::from_secs(5));
        } else {
            break;
        }
    }
}


fn show_toast_notification(title: String, body_message: String, logo: Option<String>) {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let logo_file = if let Some(logo_name) = logo {
        if !logo_name.trim().is_empty() {
            current_dir.join("images").join(logo_name)
        } else {
            current_dir.join("images").join("default_toast_logo.png")
        }
    } else {
        current_dir.join("images").join("default_toast_logo.png")
    };
    WinToastNotify::new()
        .set_title(&title)
        .set_logo(logo_file.to_str().expect("Path is an invalid unicode"), CropCircle::True)
        .set_messages(vec![&body_message])
        .show()
        .expect("Failed to show toast notification");
}



fn main() {
    let config = load_config(); // Load the configuration from the config.json file
    // Spawn a thread to run the MQTT client
    std::thread::spawn(move || {
        start_mqtt_client(config); // Pass the config to the MQTT client
    });

    // Start the system tray
    start_tray();
}
