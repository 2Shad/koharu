use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::ExportTarget;

const UNAVAILABLE: &str = "Koharu Android platform services are unavailable on this platform";

#[derive(Clone)]
pub struct Android;

impl Android {
    pub async fn pick_import(&self, _extensions: &[&str]) -> Result<Option<Vec<PathBuf>>> {
        bail!(UNAVAILABLE)
    }

    pub async fn pick_export_directory(&self, _suggested_name: &str) -> Result<Option<String>> {
        bail!(UNAVAILABLE)
    }

    pub async fn pick_export_file(
        &self,
        _suggested_name: &str,
        _mime: &str,
    ) -> Result<Option<String>> {
        bail!(UNAVAILABLE)
    }

    pub async fn publish_export(&self, _target: &ExportTarget, _paths: Vec<PathBuf>) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub async fn share(&self, _paths: Vec<PathBuf>, _mime: &str) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub fn secret_get(&self, _key: &str) -> Result<Option<String>> {
        bail!(UNAVAILABLE)
    }

    pub fn secret_set(&self, _key: &str, _value: &str) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub fn secret_delete(&self, _key: &str) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub async fn background(&self) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub async fn foreground_start(&self, _title: &str, _text: &str) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub async fn foreground_update(
        &self,
        _title: &str,
        _text: &str,
        _progress: Option<(u32, u32)>,
    ) -> Result<()> {
        bail!(UNAVAILABLE)
    }

    pub async fn foreground_stop(&self) -> Result<()> {
        bail!(UNAVAILABLE)
    }
}
