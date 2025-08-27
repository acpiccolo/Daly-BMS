use anyhow::{bail, Context, Result};
use dalybms_lib::protocol;
use dalybms_lib::serialport::DalyBMS;
use log::{error, info, warn};
use serde_json::json;
use std::collections::HashMap;

use crate::{commandline, mqtt};

#[derive(Debug)]
enum FetchedData {
    Status(protocol::Status),
    Soc(protocol::Soc),
    Mosfet(protocol::MosfetStatus),
    CellVoltageRange(protocol::CellVoltageRange),
    TemperatureRange(protocol::TemperatureRange),
    CellVoltages(protocol::CellVoltages),
    CellTemperatures(Vec<i32>),
    Balancing(Vec<bool>),
    Errors(Vec<protocol::ErrorCode>),
}

impl FetchedData {
    fn to_json_value(&self) -> Result<serde_json::Value> {
        match self {
            FetchedData::Status(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::Soc(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::Mosfet(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::CellVoltageRange(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::TemperatureRange(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::CellVoltages(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::CellTemperatures(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::Balancing(s) => serde_json::to_value(s).map_err(Into::into),
            FetchedData::Errors(s) => serde_json::to_value(s).map_err(Into::into),
        }
    }

    fn as_debug_string(&self) -> String {
        match self {
            FetchedData::Status(s) => format!("{s:?}"),
            FetchedData::Soc(s) => format!("{s:?}"),
            FetchedData::Mosfet(s) => format!("{s:?}"),
            FetchedData::CellVoltageRange(s) => format!("{s:?}"),
            FetchedData::TemperatureRange(s) => format!("{s:?}"),
            FetchedData::CellVoltages(s) => format!("{s:?}"),
            FetchedData::CellTemperatures(s) => format!("{s:?}"),
            FetchedData::Balancing(s) => format!("{s:?}"),
            FetchedData::Errors(s) => format!("{s:?}"),
        }
    }
}

struct Metric<'a> {
    fetch: Box<dyn Fn(&mut DalyBMS) -> Result<FetchedData>>,
    dependencies: &'a [&'a str],
}

fn get_metrics<'a>() -> HashMap<&'a str, Metric<'a>> {
    let mut metrics: HashMap<&'a str, Metric<'a>> = HashMap::new();
    metrics.insert(
        "status",
        Metric {
            fetch: Box::new(|bms| Ok(bms.get_status().map(FetchedData::Status)?)),
            dependencies: &[],
        },
    );
    metrics.insert(
        "soc",
        Metric {
            fetch: Box::new(|bms| Ok(bms.get_soc().map(FetchedData::Soc)?)),
            dependencies: &[],
        },
    );
    metrics.insert(
        "mosfet",
        Metric {
            fetch: Box::new(|bms| Ok(bms.get_mosfet_status().map(FetchedData::Mosfet)?)),
            dependencies: &[],
        },
    );
    metrics.insert(
        "voltage-range",
        Metric {
            fetch: Box::new(|bms| {
                Ok(bms
                    .get_cell_voltage_range()
                    .map(FetchedData::CellVoltageRange)?)
            }),
            dependencies: &[],
        },
    );
    metrics.insert(
        "temperature-range",
        Metric {
            fetch: Box::new(|bms| {
                Ok(bms
                    .get_temperature_range()
                    .map(FetchedData::TemperatureRange)?)
            }),
            dependencies: &[],
        },
    );
    metrics.insert(
        "cell-voltages",
        Metric {
            fetch: Box::new(|bms| Ok(bms.get_cell_voltages().map(FetchedData::CellVoltages)?)),
            dependencies: &["status"],
        },
    );
    metrics.insert(
        "cell-temperatures",
        Metric {
            fetch: Box::new(|bms| {
                Ok(bms
                    .get_cell_temperatures()
                    .map(FetchedData::CellTemperatures)?)
            }),
            dependencies: &["status"],
        },
    );
    metrics.insert(
        "balancing",
        Metric {
            fetch: Box::new(|bms| Ok(bms.get_balancing_status().map(FetchedData::Balancing)?)),
            dependencies: &[],
        },
    );
    metrics.insert(
        "errors",
        Metric {
            fetch: Box::new(|bms| Ok(bms.get_errors().map(FetchedData::Errors)?)),
            dependencies: &[],
        },
    );
    metrics
}

fn publish_simple_format(
    publisher: &mqtt::MqttPublisher,
    base_topic: &str,
    metric_name: &str,
    value: &serde_json::Value,
) {
    fn publish_recursive(publisher: &mqtt::MqttPublisher, topic: &str, val: &serde_json::Value) {
        match val {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let sub_topic = format!("{topic}/{k}");
                    publish_recursive(publisher, &sub_topic, v);
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let sub_topic = format!("{topic}/{i}");
                    publish_recursive(publisher, &sub_topic, v);
                }
            }
            serde_json::Value::String(s) => {
                if let Err(e) = publisher.publish(topic, s) {
                    error!("Failed to publish message to topic {topic}: {e}");
                }
            }
            serde_json::Value::Number(n) => {
                if let Err(e) = publisher.publish(topic, &n.to_string()) {
                    error!("Failed to publish message to topic {topic}: {e}");
                }
            }
            serde_json::Value::Bool(b) => {
                if let Err(e) = publisher.publish(topic, &b.to_string()) {
                    error!("Failed to publish message to topic {topic}: {e}");
                }
            }
            serde_json::Value::Null => {
                // Do not publish null values
            }
        }
    }
    let root_topic = format!("{base_topic}/{metric_name}");
    publish_recursive(publisher, &root_topic, value);
}

pub fn run(
    mut bms: DalyBMS,
    output: commandline::DaemonOutput,
    interval: std::time::Duration,
    metrics_to_fetch: Vec<String>,
) -> Result<()> {
    info!(
        "Starting daemon mode: output={output:?}, interval={interval:?}, metrics={metrics_to_fetch:?}"
    );
    let available_metrics = get_metrics();

    let mut mqtt_publisher: Option<mqtt::MqttPublisher> = None;

    if let commandline::DaemonOutput::Mqtt { config_file, .. } = &output {
        let config = mqtt::MqttConfig::load(config_file)
            .with_context(|| format!("Failed to open MQTT config file at '{config_file}'"))?;
        info!("Successfully loaded MQTT config from {config_file}: {config:?}");
        let publisher =
            mqtt::MqttPublisher::new(config).with_context(|| "Failed to create MQTT publisher")?;
        info!("MQTT Publisher created successfully.");
        mqtt_publisher = Some(publisher);
    }

    loop {
        let mut fetched_data: HashMap<String, FetchedData> = HashMap::new();
        let mut metrics_to_process = metrics_to_fetch.clone();

        if metrics_to_process.iter().any(|m| m == "all") {
            info!("Fetching all metrics due to 'all' flag.");
            metrics_to_process = available_metrics.keys().map(|s| s.to_string()).collect();
        }

        for metric_name in &metrics_to_process {
            if let Some(metric) = available_metrics.get(metric_name.as_str()) {
                for &dep in metric.dependencies {
                    if !fetched_data.contains_key(dep)
                        && metrics_to_process.contains(&dep.to_string())
                    {
                        if let Some(dep_metric) = available_metrics.get(dep) {
                            info!("Fetching dependency '{dep}' for '{metric_name}'");
                            match (dep_metric.fetch)(&mut bms) {
                                Ok(data) => {
                                    fetched_data.insert(dep.to_string(), data);
                                }
                                Err(e) => error!("Error fetching dependency '{dep}': {e}"),
                            }
                        }
                    }
                }
                info!("Fetching metric: {metric_name}");
                match (metric.fetch)(&mut bms) {
                    Ok(data) => {
                        fetched_data.insert(metric_name.to_string(), data);
                    }
                    Err(e) => error!("Error fetching metric '{metric_name}': {e}"),
                }
            } else {
                bail!("Unknown metric name '{}'", metric_name);
            }
        }

        match &output {
            commandline::DaemonOutput::Console => {
                println!("--- Data at {} ---", chrono::Local::now().to_rfc3339());
                for (name, data) in &fetched_data {
                    println!("{}: {}", name, data.as_debug_string());
                }
                println!("--------------------------");
            }
            commandline::DaemonOutput::Mqtt { format, .. } => {
                if let Some(publisher) = &mqtt_publisher {
                    match format {
                        commandline::MqttFormat::Json => {
                            let mut data_to_publish = serde_json::Map::new();
                            data_to_publish.insert(
                                "timestamp".to_string(),
                                json!(chrono::Utc::now().to_rfc3339()),
                            );

                            for (name, data) in &fetched_data {
                                match data.to_json_value() {
                                    Ok(val) => {
                                        data_to_publish.insert(name.clone(), val);
                                    }
                                    Err(e) => error!("Failed to serialize '{name}': {e}"),
                                }
                            }

                            if data_to_publish.len() > 1 {
                                match serde_json::to_string(&data_to_publish) {
                                    Ok(json_payload) => {
                                        info!(
                                            "MQTT output: Attempting to publish data: {json_payload}"
                                        );
                                        if let Err(e) =
                                            publisher.publish(publisher.topic(), &json_payload)
                                        {
                                            error!("Failed to publish data to MQTT: {e:?}");
                                        } else {
                                            info!("Successfully published data to MQTT.");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize data to JSON string: {e}");
                                    }
                                }
                            } else {
                                info!("No data fetched in this cycle to publish via MQTT.");
                            }
                        }
                        commandline::MqttFormat::Simple => {
                            let base_topic = publisher.topic();
                            for (name, data) in &fetched_data {
                                match data.to_json_value() {
                                    Ok(value) => {
                                        publish_simple_format(publisher, base_topic, name, &value);
                                    }
                                    Err(e) => error!("Failed to serialize '{name}': {e}"),
                                }
                            }
                        }
                    }
                } else {
                    warn!(
                        "MQTT output selected, but publisher is not initialized. Skipping publish."
                    );
                }
            }
        }
        std::thread::sleep(interval);
    }
}
