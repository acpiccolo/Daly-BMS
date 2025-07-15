use anyhow::{Context, Result};
use log::{info, warn};
use paho_mqtt::{
    Client, ConnectOptionsBuilder, CreateOptionsBuilder, MessageBuilder, QoS,
};
use serde::Deserialize;
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
        let mut client_id_base = self
            .config
            .client_id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| {
                default_client_id_prefix().unwrap_or_else(|| "dalybms-rs".to_string())
            });

        // Append a random suffix if the default prefix was used or if client_id was None/empty
        if self.config.client_id.is_none()
            || self.config.client_id.as_deref() == Some("dalybms-tool")
            || self.config.client_id.as_deref() == Some("")
        {
            client_id_base = format!("{}-{}", client_id_base, generate_random_string(8));
        }
        let client_id = client_id_base; // Final client_id

        // Construct server URI, always TCP for non-SSL
        let server_uri = format!("tcp://{}:{}", self.config.server, self.config.port);

        let create_opts = CreateOptionsBuilder::new()
            .server_uri(&server_uri)
            .client_id(&client_id)
            .persistence(None) // In-memory persistence
            .finalize();

        let client = Client::new(create_opts)
            .with_context(|| format!("Error creating MQTT client for server: {}", server_uri))?;

        let mut conn_opts_builder = ConnectOptionsBuilder::new();
        conn_opts_builder.keep_alive_interval(Duration::from_secs(20)); // Paho default is 60s, rumqttc was 5s
        // conn_opts_builder.automatic_reconnect(false); // Explicitly disable auto-reconnect for publish-once
        // For paho-mqtt 0.13.3, automatic_reconnect takes two Durations (min, max retry interval)
        // Default is disabled, so commenting it out achieves the desired behavior for publish-once.

        if let (Some(username), Some(password)) = (&self.config.username, &self.config.password) {
            conn_opts_builder.user_name(username);
            conn_opts_builder.password(password);
        } else if self.config.username.is_some() {
            warn!("MQTT username provided without a password. Connecting without authentication.");
        }

        // SSL Options removed
        // if use_ssl {
        //     let ssl_options = SslOptionsBuilder::new()
        //         // .trust_store("certs/ca.crt")? // Example: if you need custom CA
        //         // .key_store("certs/client.crt")?
        //         // .private_key("certs/client.key")?
        //         .enable_server_cert_auth(true) // Validate server certificate
        //         .verify(true) // Verify server hostname
        //         .finalize();
        //     conn_opts_builder.ssl_options(ssl_options);
        // }

        let conn_opts = conn_opts_builder.finalize();

        info!(
            "Attempting to connect to MQTT broker: {} with client_id: {}",
            server_uri, client_id
        );

        client
            .connect(conn_opts)
            .with_context(|| "Failed to connect to MQTT broker")?;
        info!("Connected to MQTT broker.");

        let topic = &self.config.topic;
        let msg = MessageBuilder::new()
            .topic(topic)
            .payload(payload)
            .qos(QoS::AtLeastOnce)
            .retained(false)
            .finalize();

        client
            .publish(msg)
            .with_context(|| format!("Failed to publish message to MQTT topic: {}", topic))?;

        info!("Message published to topic: {}. For QoS > 0, sync client typically waits for ack.", topic);
        // With paho-mqtt sync client, for QoS 1 and 2, publish() blocks until the handshake is complete.
        // So, if publish returns Ok(()), the message has been acknowledged by the broker.
        // No explicit delivery_token.wait_for_completion_timeout() is needed here.

        client
            .disconnect(None) // None uses default disconnect options (e.g., 10 sec timeout)
            .with_context(|| "Failed to disconnect MQTT client")?;
        info!("Disconnected from MQTT broker.");

        Ok(())
    }
}

fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    use rand::distr::Alphanumeric;

    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
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
