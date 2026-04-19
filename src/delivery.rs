use crate::config::DeliveryConfig;
use anyhow::Context;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryOutput {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub trait Sender {
    fn send(&mut self, message: &str) -> anyhow::Result<DeliveryOutput>;
}

pub struct ImsgSender<'a> {
    config: &'a DeliveryConfig,
}

impl<'a> ImsgSender<'a> {
    pub fn new(config: &'a DeliveryConfig) -> Self {
        Self { config }
    }
}

impl Sender for ImsgSender<'_> {
    fn send(&mut self, message: &str) -> anyhow::Result<DeliveryOutput> {
        send_imsg(self.config, message)
    }
}

pub fn send_imsg(config: &DeliveryConfig, message: &str) -> anyhow::Result<DeliveryOutput> {
    let output = Command::new(&config.imsg_path)
        .arg("send")
        .arg("--to")
        .arg(&config.destination)
        .arg("--text")
        .arg(message)
        .output()
        .with_context(|| format!("failed to execute {}", config.imsg_path.display()))?;

    Ok(DeliveryOutput {
        status_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}
