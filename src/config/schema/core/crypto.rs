use super::Config;
use crate::security::SecretStore;
use anyhow::Result;
use std::path::Path;

fn decrypt_secret_string(
    value: &mut String,
    store: &SecretStore,
    encrypt_enabled: bool,
) -> Result<bool> {
    let current = value.trim();
    if current.is_empty() {
        return Ok(false);
    }

    let needs_encrypt_persist = encrypt_enabled && !SecretStore::is_encrypted(current);
    let (decrypted, migrated) = store.decrypt_and_migrate(current)?;
    *value = decrypted;

    Ok(needs_encrypt_persist || migrated.is_some())
}

fn decrypt_secret_option(
    value: &mut Option<String>,
    store: &SecretStore,
    encrypt_enabled: bool,
) -> Result<bool> {
    let Some(current) = value.as_deref() else {
        return Ok(false);
    };

    let trimmed = current.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let needs_encrypt_persist = encrypt_enabled && !SecretStore::is_encrypted(trimmed);
    let (decrypted, migrated) = store.decrypt_and_migrate(trimmed)?;
    *value = Some(decrypted);

    Ok(needs_encrypt_persist || migrated.is_some())
}

fn encrypt_secret_string(value: &mut String, store: &SecretStore) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || SecretStore::is_encrypted(trimmed) {
        if trimmed != value {
            *value = trimmed.to_string();
        }
        return Ok(());
    }

    *value = store.encrypt(trimmed)?;
    Ok(())
}

fn encrypt_secret_option(value: &mut Option<String>, store: &SecretStore) -> Result<()> {
    let Some(current) = value.as_deref() else {
        return Ok(());
    };

    let trimmed = current.trim();
    if trimmed.is_empty() || SecretStore::is_encrypted(trimmed) {
        if trimmed != current {
            *value = Some(trimmed.to_string());
        }
        return Ok(());
    }

    *value = Some(store.encrypt(trimmed)?);
    Ok(())
}

impl Config {
    fn secret_store_root(&self) -> &Path {
        self.config_path.parent().unwrap_or_else(|| Path::new("."))
    }

    fn secret_store(&self) -> SecretStore {
        SecretStore::new(self.secret_store_root(), self.secrets.encrypt)
    }

    pub(super) fn decrypt_config_secrets_in_place(&mut self) -> Result<bool> {
        let store = self.secret_store();
        let mut needs_persist = false;

        needs_persist |= decrypt_secret_option(&mut self.api_key, &store, self.secrets.encrypt)?;
        needs_persist |=
            decrypt_secret_option(&mut self.composio.api_key, &store, self.secrets.encrypt)?;

        if let Some(telegram) = self.channels_config.telegram.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut telegram.bot_token, &store, self.secrets.encrypt)?;
        }

        if let Some(discord) = self.channels_config.discord.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut discord.bot_token, &store, self.secrets.encrypt)?;
        }

        if let Some(slack) = self.channels_config.slack.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut slack.bot_token, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut slack.app_token, &store, self.secrets.encrypt)?;
        }

        if let Some(webhook) = self.channels_config.webhook.as_mut() {
            needs_persist |=
                decrypt_secret_option(&mut webhook.secret, &store, self.secrets.encrypt)?;
        }

        if let Some(matrix) = self.channels_config.matrix.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut matrix.access_token, &store, self.secrets.encrypt)?;
        }

        if let Some(whatsapp) = self.channels_config.whatsapp.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut whatsapp.access_token, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_string(&mut whatsapp.verify_token, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut whatsapp.app_secret, &store, self.secrets.encrypt)?;
        }

        if let Some(irc) = self.channels_config.irc.as_mut() {
            needs_persist |=
                decrypt_secret_option(&mut irc.server_password, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut irc.nickserv_password, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut irc.sasl_password, &store, self.secrets.encrypt)?;
        }

        if let Some(cloudflare) = self.tunnel.cloudflare.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut cloudflare.token, &store, self.secrets.encrypt)?;
        }

        if let Some(ngrok) = self.tunnel.ngrok.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut ngrok.auth_token, &store, self.secrets.encrypt)?;
        }

        Ok(needs_persist)
    }

    pub(super) fn encrypt_config_secrets_in_place(&mut self) -> Result<()> {
        if !self.secrets.encrypt {
            return Ok(());
        }

        let store = self.secret_store();

        encrypt_secret_option(&mut self.api_key, &store)?;
        encrypt_secret_option(&mut self.composio.api_key, &store)?;

        if let Some(telegram) = self.channels_config.telegram.as_mut() {
            encrypt_secret_string(&mut telegram.bot_token, &store)?;
        }

        if let Some(discord) = self.channels_config.discord.as_mut() {
            encrypt_secret_string(&mut discord.bot_token, &store)?;
        }

        if let Some(slack) = self.channels_config.slack.as_mut() {
            encrypt_secret_string(&mut slack.bot_token, &store)?;
            encrypt_secret_option(&mut slack.app_token, &store)?;
        }

        if let Some(webhook) = self.channels_config.webhook.as_mut() {
            encrypt_secret_option(&mut webhook.secret, &store)?;
        }

        if let Some(matrix) = self.channels_config.matrix.as_mut() {
            encrypt_secret_string(&mut matrix.access_token, &store)?;
        }

        if let Some(whatsapp) = self.channels_config.whatsapp.as_mut() {
            encrypt_secret_string(&mut whatsapp.access_token, &store)?;
            encrypt_secret_string(&mut whatsapp.verify_token, &store)?;
            encrypt_secret_option(&mut whatsapp.app_secret, &store)?;
        }

        if let Some(irc) = self.channels_config.irc.as_mut() {
            encrypt_secret_option(&mut irc.server_password, &store)?;
            encrypt_secret_option(&mut irc.nickserv_password, &store)?;
            encrypt_secret_option(&mut irc.sasl_password, &store)?;
        }

        if let Some(cloudflare) = self.tunnel.cloudflare.as_mut() {
            encrypt_secret_string(&mut cloudflare.token, &store)?;
        }

        if let Some(ngrok) = self.tunnel.ngrok.as_mut() {
            encrypt_secret_string(&mut ngrok.auth_token, &store)?;
        }

        Ok(())
    }

    pub(super) fn config_for_persistence(&self) -> Result<Self> {
        let mut persisted = self.clone();
        persisted.encrypt_config_secrets_in_place()?;
        Ok(persisted)
    }
}
