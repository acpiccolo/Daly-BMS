use anyhow::{Context, Result};
use log::{info, warn};
use rumqttc::{Client, MqttOptions, QoS};
use serde::Deserialize; // Reverted to normal path
use std::fs;
use std::time::Duration; // Added for logging

#[derive(Debug, Deserialize, Clone)] // Added Clone for storing config
pub struct MqttConfig {
    pub server: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic: String,
    #[serde(default = "default_client_id_prefix")]
    pub client_id: Option<String>, // Will be prefixed if None or empty
}

fn default_port() -> u16 {
    1883
}

fn default_client_id_prefix() -> Option<String> {
    Some("dalybms-tool".to_string())
}

pub fn load_mqtt_config(path: &str) -> Result<MqttConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read MQTT config file from path: {path}"))?;
    let config: MqttConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse MQTT config from file: {path}"))?;
    Ok(config)
}

pub struct MqttPublisher {
    config: MqttConfig,
}

impl MqttPublisher {
    pub fn new(config: MqttConfig) -> Result<Self> {
        // No connection is established here, only config is stored.
        Ok(Self { config })
    }

    pub fn publish(&self, payload: &str) -> Result<()> {
        let mut client_id = self
            .config
            .client_id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| {
                default_client_id_prefix().unwrap_or_else(|| "dalybms-rs".to_string())
            });

        // Append a random suffix if the default prefix was used or if client_id was None
        if self.config.client_id.is_none()
            || self.config.client_id.as_deref() == Some("dalybms-tool")
        {
            client_id = format!("{}-{}", client_id, generate_random_string(8));
        }

        let mut mqttoptions = MqttOptions::new(client_id, &self.config.server, self.config.port);
        mqttoptions.set_keep_alive(Duration::from_secs(5));

        if let (Some(username), Some(password)) = (&self.config.username, &self.config.password) {
            mqttoptions.set_credentials(username, password);
        } else if self.config.username.is_some() {
            warn!("MQTT username provided without a password. Connecting without authentication.");
        }

        // Enable TCP transport encryption if server is "ssl://..." or similar
        // This is a basic check, real-world might need more robust parsing or explicit config field
        if self.config.server.starts_with("ssl://") || self.config.server.starts_with("mqtts://") {
            // The server string should be just the host then, e.g. "test.mosquitto.org"
            // For now, rumqttc handles this if the server address is correct and compiled with TLS features.
            // We might need to add `native-tls` or `rustls` feature to rumqttc if not default.
            // Assuming `rumqttc` is built with TLS support for this example.
            // If using `ssl://` prefix, `rumqttc` might expect `host` without prefix for `MqttOptions::new`
            // and then use `set_transport(Transport::tls(...))`
            // For now, let's assume the user provides the correct host and port, and if TLS is needed,
            // it's handled by the server string or a more explicit config later.
            // The `rumqttc` simple example uses host directly and TLS is often a feature flag.
            // For this iteration, we won't explicitly set Transport::tls unless issues arise.
            // If issues, will need to adjust MqttOptions setup for TLS.
        }

        info!(
            "Attempting to connect to MQTT broker: {}:{}",
            self.config.server, self.config.port
        );
        let (mut client, mut connection) = Client::new(mqttoptions, 10);

        // The event loop needs to be polled for the connection to work and messages to be acknowledged.
        // In a sync context for a simple publish, we can make a few iterations.
        // This is a point from documentation: "Connection hosts its own thread via connect method."
        // "Client is a handle to work with the connection thread."
        // For the synchronous client, `Client::new` creates a client and an eventloop (`connection`).
        // The `connection.iter()` or manual poll is essential.

        // A simple way for "publish and forget" without a dedicated thread for eventloop:
        // 1. Connect (implicitly done by Client::new + first poll)
        // 2. Publish
        // 3. Wait a very short moment for publish to go through (e.g. by polling a few times or a small sleep)
        // 4. Disconnect

        // According to rumqttc docs, for sync client:
        // "The event loop is usually iterated until 'Disconnected' event is received."
        // "publish() might block if outgoing buffer is full."
        // For a robust single publish, we should ensure the event loop runs a bit.

        // Let's try to publish and then explicitly poll the connection a few times to ensure
        // the message is sent and connection handles its state, then disconnect.
        // This is a common pattern for tools that publish intermittently.

        let topic = &self.config.topic;
        client
            .publish(topic, QoS::AtLeastOnce, false, payload.as_bytes())
            .with_context(|| format!("Failed to publish message to MQTT topic: {topic}"))?;
        info!("Published message to topic: {}", topic);

        // Poll the event loop a few times to allow the client to process the publish
        // and handle any immediate responses or errors.
        for _ in 0..5 {
            // Arbitrary number of polls
            match connection.recv_timeout(Duration::from_millis(10)) {
                Ok(_notification) => {
                    // Process notification if needed, for publish usually not critical unless checking for PubAck
                    // info!("MQTT Notification: {:?}", _notification);
                    // Potentially break if a relevant ack is received, or just continue polling.
                }
                Err(e) => {
                    // More generic error handling for the poll
                    warn!(
                        "Error/Timeout during MQTT connection poll after publish: {:?}",
                        e
                    );
                    // Break on any error or timeout during this brief poll.
                    break;
                }
            }
        }

        client
            .disconnect()
            .with_context(|| "Failed to disconnect MQTT client")?;
        info!("Disconnected from MQTT broker.");

        Ok(())
    }
}

fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_mqtt_config_valid() {
        let yaml_content = r#"
server: "mqtt.example.com"
port: 1884
username: "testuser"
password: "testpassword"
topic: "dalybms/test"
client_id: "test_client_001"
        "#;
        let mut tmpfile = NamedTempFile::new().unwrap();
        write!(tmpfile, "{}", yaml_content).unwrap();

        let config_result = load_mqtt_config(tmpfile.path().to_str().unwrap());
        assert!(config_result.is_ok());
        let config = config_result.unwrap();

        assert_eq!(config.server, "mqtt.example.com");
        assert_eq!(config.port, 1884);
        assert_eq!(config.username, Some("testuser".to_string()));
        assert_eq!(config.password, Some("testpassword".to_string()));
        assert_eq!(config.topic, "dalybms/test");
        assert_eq!(config.client_id, Some("test_client_001".to_string()));
    }

    #[test]
    fn test_load_mqtt_config_invalid_yaml() {
        let yaml_content = "server: mqtt.example.com\nport: 1883\n invalid_yaml_at_this_level";
        let mut tmpfile = NamedTempFile::new().unwrap();
        write!(tmpfile, "{}", yaml_content).unwrap();

        let config_result = load_mqtt_config(tmpfile.path().to_str().unwrap());
        assert!(config_result.is_err());
        // Optionally, check for specific error kind if anyhow allows/makes it easy
        // For now, just checking it's an error is sufficient for this test's purpose.
    }

    #[test]
    fn test_load_mqtt_config_missing_required_fields() {
        // Missing 'server'
        let yaml_content_missing_server = r#"
port: 1883
topic: "dalybms/test/missing_server"
        "#;
        let mut tmpfile_server = NamedTempFile::new().unwrap();
        write!(tmpfile_server, "{}", yaml_content_missing_server).unwrap();
        let config_result_server = load_mqtt_config(tmpfile_server.path().to_str().unwrap());
        assert!(config_result_server.is_err());

        // Missing 'topic'
        let yaml_content_missing_topic = r#"
server: "mqtt.example.com"
port: 1883
        "#;
        let mut tmpfile_topic = NamedTempFile::new().unwrap();
        write!(tmpfile_topic, "{}", yaml_content_missing_topic).unwrap();
        let config_result_topic = load_mqtt_config(tmpfile_topic.path().to_str().unwrap());
        assert!(config_result_topic.is_err());
    }

    #[test]
    fn test_load_mqtt_config_default_values() {
        let yaml_content = r#"
server: "mqtt.default.com"
topic: "dalybms/default_topic"
# port is omitted
# client_id is omitted
# username and password omitted
        "#;
        let mut tmpfile = NamedTempFile::new().unwrap();
        write!(tmpfile, "{}", yaml_content).unwrap();

        let config_result = load_mqtt_config(tmpfile.path().to_str().unwrap());
        assert!(config_result.is_ok());
        let config = config_result.unwrap();

        assert_eq!(config.server, "mqtt.default.com");
        assert_eq!(config.port, 1883); // Check default port
        assert_eq!(config.topic, "dalybms/default_topic");
        assert_eq!(config.username, None);
        assert_eq!(config.password, None);
        // client_id in MqttConfig has #[serde(default = "default_client_id_prefix")]
        // which gives Some("dalybms-tool") if client_id is missing from yaml
        assert_eq!(config.client_id, Some("dalybms-tool".to_string()));
    }

    #[test]
    fn test_load_mqtt_config_client_id_empty_uses_default_prefix() {
        // Test that if client_id is present but empty, it still uses the default prefix logic.
        // The publisher logic appends a random string if the client_id is the default_prefix.
        let yaml_content = r#"
server: "mqtt.default.com"
topic: "dalybms/default_topic"
client_id: ""
        "#;
        let mut tmpfile = NamedTempFile::new().unwrap();
        write!(tmpfile, "{}", yaml_content).unwrap();

        let config_result = load_mqtt_config(tmpfile.path().to_str().unwrap());
        assert!(config_result.is_ok());
        let config = config_result.unwrap();
        // If client_id is empty in YAML, serde deserializes it as Some("").
        // The default_client_id_prefix is only used if the key is entirely missing.
        assert_eq!(config.client_id, Some("".to_string()));
    }
}
