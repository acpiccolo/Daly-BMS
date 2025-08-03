use anyhow::{Context, Result};
use paho_mqtt::{Client, ConnectOptionsBuilder, CreateOptionsBuilder, MessageBuilder};
use serde::Deserialize;
use std::fs;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct MqttConfig {
    uri: String,
    username: Option<String>,
    password: Option<String>,
    #[serde(default = "default_topic")]
    topic: String,
    #[serde(default = "default_qos")]
    qos: i32,
    #[serde(default = "default_client_id")]
    client_id: String,
    #[serde(default = "default_operation_timeout")]
    oparation_timeout: Duration,
    #[serde(default = "default_keep_alive_interval")]
    keep_alive_interval: Duration,
    #[serde(default = "default_auto_reconnect_interval_min")]
    auto_reconnect_interval_min: Duration,
    #[serde(default = "default_auto_reconnect_interval_max")]
    auto_reconnect_interval_max: Duration,
}

fn default_topic() -> String {
    "dalybms".into()
}

fn default_qos() -> i32 {
    0
}

fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    use rand::distr::Alphanumeric;

    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn default_client_id() -> String {
    format!("dalybms-{}", generate_random_string(8))
}

fn default_operation_timeout() -> Duration {
    Duration::from_secs(10)
}

fn default_keep_alive_interval() -> Duration {
    Duration::from_secs(30)
}

fn default_auto_reconnect_interval_min() -> Duration {
    Duration::from_secs(1)
}

fn default_auto_reconnect_interval_max() -> Duration {
    Duration::from_secs(30)
}

pub fn load_mqtt_config(path: &str) -> Result<MqttConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read MQTT config file from path: {path}"))?;
    let config: MqttConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse MQTT config from file: {path}"))?;
    Ok(config)
}

pub struct MqttPublisher {
    client: Client,
    config: MqttConfig,
}

impl MqttPublisher {
    pub fn new(config: MqttConfig) -> Result<Self> {
        let create_opts = CreateOptionsBuilder::new()
            .server_uri(&config.uri)
            .client_id(&config.client_id)
            .persistence(None) // In-memory persistence
            .finalize();

        let mut client = Client::new(create_opts)
            .with_context(|| format!("Error creating MQTT client for server: {}", config.uri))?;

        client.set_timeout(config.oparation_timeout);

        let mut conn_builder = ConnectOptionsBuilder::new();
        conn_builder
            .keep_alive_interval(config.keep_alive_interval)
            .clean_session(true) // Typically true for telemetry publishers
            .automatic_reconnect(
                config.auto_reconnect_interval_min,
                config.auto_reconnect_interval_max,
            ); // Enable auto-reconnect

        if let Some(user_name_str) = &config.username {
            conn_builder.user_name(user_name_str.as_str());
        }
        if let Some(password_str) = &config.password {
            conn_builder.password(password_str.as_str());
        }
        let conn_opts = conn_builder.finalize();

        log::info!(
            "Attempting to connect to MQTT broker: {} with client_id: {}",
            config.uri,
            config.client_id
        );

        client
            .connect(conn_opts)
            .with_context(|| "Failed to connect to MQTT broker")?;
        log::info!("Connected to MQTT broker.");
        Ok(Self { client, config })
    }

    pub fn topic(&self) -> &str {
        &self.config.topic
    }

    pub fn publish(&self, topic: &str, payload: &str) -> Result<()> {
        let msg = MessageBuilder::new()
            .topic(topic)
            .payload(payload)
            .qos(self.config.qos)
            .retained(false)
            .finalize();

        log::debug!(
            "Publishing to MQTT: Topic='{}', Payload='{payload}', QoS={}",
            topic,
            self.config.qos
        );

        self.client
            .publish(msg)
            .with_context(|| format!("Failed to publish message to MQTT topic: {}", topic))?;

        Ok(())
    }
}
