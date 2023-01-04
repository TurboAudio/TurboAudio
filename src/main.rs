mod audio;
mod config_parser;
mod connections;
mod pipewire_listener;
mod resources;
use resources::{
    color::Color,
    effects::{moody::update_moody, raindrop::update_raindrop},
    ledstrip::LedStrip,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddrV4},
};

use anyhow::{anyhow, Result};
use audio::start_audio_loop;
use clap::Parser;
use config_parser::TurboAudioConfig;
use connections::{tcp::TcpConnection, usb::UsbConnection, Connection};
use pipewire_listener::PipewireController;

use crate::resources::{
    effects::{
        lua::{LuaEffect, LuaEffectSettings},
        moody::{Moody, MoodySettings},
        raindrop::{RaindropSettings, RaindropState, Raindrops},
        Effect,
    },
    settings::Settings,
};

#[derive(Parser, Debug)]
#[command(author, version, long_about = None)]
struct Args {
    /// Settings file
    #[arg(long, default_value_t = String::from("Settings"))]
    settings_file: String,
}

fn test_and_run_loop() {
    let mut settings: HashMap<i32, Settings> = HashMap::default();
    let mut effects: HashMap<i32, Effect> = HashMap::default();
    let mut effect_settings: HashMap<i32, i32> = HashMap::default();
    let mut connections: HashMap<i32, Connection> = HashMap::default();
    let mut ledstrips = Vec::default();

    let moody_settings = MoodySettings {
        color: Color { r: 255, g: 0, b: 0 },
    };
    let raindrop_settings = RaindropSettings {
        rain_speed: 1,
        drop_rate: 0.10,
    };
    let lua_settings = LuaEffectSettings {
        settings: serde_json::json!({
            "enable_beep_boops": true,
            "intensity": 11,
        }),
    };
    settings.insert(0, Settings::Moody(moody_settings));
    settings.insert(1, Settings::Raindrop(raindrop_settings));
    settings.insert(2, Settings::Lua(lua_settings));

    let moody = Moody { id: 10 };
    effects.insert(10, Effect::Moody(moody));
    effect_settings.insert(10, 0);

    let raindrop = Raindrops {
        id: 20,
        state: RaindropState { riples: vec![] },
    };
    effects.insert(20, Effect::Raindrop(raindrop));
    effect_settings.insert(20, 1);

    let lua_effect = match LuaEffect::new("scripts/fade.lua") {
        Ok(effect) => effect,
        Err(e) => {
            eprint!("Error: {:?}", e);
            return;
        }
    };
    effects.insert(30, Effect::Lua(lua_effect));
    effect_settings.insert(30, 2);

    let ip = std::net::SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 200), 1234));
    let connection = TcpConnection::new(ip);
    let connection_id = 1;
    connections.insert(connection_id, Connection::Tcp(connection));
    connections.insert(2, Connection::Usb(UsbConnection {}));

    let mut ls1 = LedStrip::default();
    ls1.set_led_count(300);
    ls1.add_effect(30, 300);
    ls1.connection_id = Some(connection_id);
    ledstrips.push(ls1);

    loop {
        std::thread::sleep(std::time::Duration::from_millis(16));
        tick(
            &mut ledstrips,
            &mut effects,
            &settings,
            &effect_settings,
            &mut connections,
        );
    }
}

fn send_to_connection(
    ledstrip: &mut LedStrip,
    connection_id: i32,
    connections: &mut HashMap<i32, Connection>,
) -> anyhow::Result<()> {
    let connection = connections
        .get_mut(&connection_id)
        .ok_or_else(|| anyhow!("Connection id {} doesn't exist", connection_id))?;

    let data = ledstrip
        .colors
        .iter()
        .flat_map(|color| color.to_bytes())
        .collect::<Vec<_>>();
    match connection {
        Connection::Tcp(tcp_connection) => {
            // If send fails, connection is closed.
            if let Err(error) = tcp_connection.data_queue.send(data) {
                eprintln!("{:?}", error);
                connections.remove(&connection_id);
                ledstrip.connection_id = None;
            };
            Ok(())
        }
        Connection::Usb(_terminal) => {
            todo!("Implement Usb connection");
        }
    }
}

// TODO: Split into 2 functions. (update_effects, send_colors)
fn tick(
    ledstrips: &mut Vec<LedStrip>,
    effects: &mut HashMap<i32, Effect>,
    settings: &HashMap<i32, Settings>,
    effect_settings: &HashMap<i32, i32>,
    connections: &mut HashMap<i32, Connection>,
) {
    for ledstrip in ledstrips {
        for (effect_id, interval) in &ledstrip.effects {
            let leds = ledstrip
                .colors
                .get_mut(interval.0..=interval.1)
                .expect("Ledstrip interval out of bounds");
            let effect = effects
                .get_mut(effect_id)
                .expect("Effect id was not found.");
            let setting_id = effect_settings
                .get(effect_id)
                .expect("Setting id not found");
            let setting = settings.get(setting_id);
            match (effect, setting) {
                (Effect::Moody(_moody), Some(Settings::Moody(settings))) => {
                    update_moody(leds, settings);
                }
                (Effect::Raindrop(raindrop), Some(Settings::Raindrop(settings))) => {
                    update_raindrop(leds, settings, &mut raindrop.state);
                }
                (Effect::Lua(lua), Some(Settings::Lua(settings))) => {
                    if let Err(e) = lua.tick(leds, settings) {
                        eprintln!("Error when executing lua function: {:?}", e);
                    }
                }
                _ => panic!("Effect doesn't match settings"),
            }
        }

        if let Some(connection_id) = ledstrip.connection_id {
            if let Err(e) = send_to_connection(ledstrip, connection_id, connections) {
                eprintln!("{:?}", e);
            }
        }
    }
}

fn main() -> Result<()> {
    let Args { settings_file } = Args::parse();
    let TurboAudioConfig {
        device_name,
        jack,
        sample_rate,
        stream_connections,
    } = TurboAudioConfig::new(&settings_file)?;

    let (_stream, _rx) = start_audio_loop(device_name, jack, sample_rate.try_into().unwrap())?;
    let pipewire_controller = PipewireController::new();
    pipewire_controller.set_stream_connections(stream_connections)?;
    test_and_run_loop();
    Ok(())
}
