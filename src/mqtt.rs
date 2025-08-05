use anyhow::{Context, Result};
use paho_mqtt::{Client, ConnectOptionsBuilder, CreateOptionsBuilder, MessageBuilder};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct MqttConfig {
    uri: String,
    username: Option<String>,
    password: Option<String>,
    #[serde(default = "MqttConfig::default_topic")]
    topic: String,
    #[serde(default = "MqttConfig::default_qos")]
    qos: i32,
    #[serde(default = "MqttConfig::default_client_id")]
    client_id: String,
    #[serde(
        default = "MqttConfig::default_operation_timeout",
        with = "humantime_serde"
    )]
    oparation_timeout: Duration,
    #[serde(
        default = "MqttConfig::default_keep_alive_interval",
        with = "humantime_serde"
    )]
    keep_alive_interval: Duration,
    #[serde(
        default = "MqttConfig::default_auto_reconnect_interval_min",
        with = "humantime_serde"
    )]
    auto_reconnect_interval_min: Duration,
    #[serde(
        default = "MqttConfig::default_auto_reconnect_interval_max",
        with = "humantime_serde"
    )]
    auto_reconnect_interval_max: Duration,
}

impl MqttConfig {
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
        format!("dalybms-{}", Self::generate_random_string(8))
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

    pub const DEFAULT_CONFIG_FILE: &str = "mqtt.yaml";

    pub fn load(config_file_path: &str) -> Result<Self> {
        log::debug!("Loading config file from {config_file_path:?}");
        let config_file = std::fs::File::open(config_file_path)
            .with_context(|| format!("Cannot open MQTT config file {config_file_path:?}"))?;
        let config: Self = serde_yaml::from_reader(&config_file)
            .with_context(|| format!("Cannot read MQTT config from file: {config_file_path:?}"))?;
        Ok(config)
    }

    pub fn create_client(&self) -> Result<Client> {
        let create_opts = CreateOptionsBuilder::new()
            .server_uri(&self.uri)
            .client_id(&self.client_id)
            .persistence(None) // In-memory persistence
            .finalize();

        let mut client = Client::new(create_opts)
            .with_context(|| format!("Error creating MQTT client for server: {}", self.uri))?;

        client.set_timeout(self.oparation_timeout);

        let mut conn_builder = ConnectOptionsBuilder::new();
        conn_builder
            .keep_alive_interval(self.keep_alive_interval)
            .clean_session(true) // Typically true for telemetry publishers
            .automatic_reconnect(
                self.auto_reconnect_interval_min,
                self.auto_reconnect_interval_max,
            ); // Enable auto-reconnect

        if let Some(user_name_str) = &self.username {
            conn_builder.user_name(user_name_str.as_str());
        }
        if let Some(password_str) = &self.password {
            conn_builder.password(password_str.as_str());
        }
        let conn_opts = conn_builder.finalize();

        log::info!(
            "Attempting to connect to MQTT broker: {} with client_id: {}",
            self.uri,
            self.client_id
        );

        client
            .connect(conn_opts)
            .with_context(|| "Failed to connect to MQTT broker")?;
        log::info!("Connected to MQTT broker.");
        Ok(client)
    }
}

pub struct MqttPublisher {
    client: Client,
    config: MqttConfig,
}

impl MqttPublisher {
    pub fn new(config: MqttConfig) -> Result<Self> {
        let client = config.create_client()?;
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
