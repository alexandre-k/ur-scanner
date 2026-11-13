use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Client, RequestBuilder};
use std::collections::HashSet;
use crate::config::CONFIG;
use crate::ur::RoomListing;

pub struct SupabaseStore {
    base_url: String,
    api_key: String,
    client: Client
}

impl SupabaseStore {
    pub fn from_config() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to build Supabase reqwest client")?;
        Ok(Self {
            base_url: format!("{}/rest/v1/notified_vacancies", CONFIG.supabase_url.trim_end_matches('/')),
            api_key: CONFIG.supabase_service_role_key.trim().to_string(),
            client
        })
    }

    fn auth(&self, req: RequestBuilder) -> RequestBuilder {
        let req = req.header("apikey", &self.api_key);
        if self.api_key.starts_with("sb_") {
            req
        } else {
            req.bearer_auth(&self.api_key)
        }
    }

    /// Fetch all currently active notified vacancies on startup
    pub async fn load_notified(&self) -> Result<HashSet<RoomListing>> {
        let req = self.client
            .get(&self.base_url)
            .query(&[("select", "estate_name,room_name,commonfee,rent,layout,floorspace,floor")]);
        let rows: Vec<RoomListing> = self
            .auth(req)
            .send()
            .await
            .context("Failed to query Supabase notified_vacancies")?
            .error_for_status()
            .context("Supabase returned error status on load")?
            .json()
            .await
            .context("Failed to deserialize Supabase rows")?;

        Ok(rows.into_iter().collect())
    }

    /// Insert (upsert) a newly notified vacancy
    pub async fn mark_notified(&self, room: &RoomListing) -> Result<()> {
        let req = self.client
            .post(&self.base_url)
            .query(&[("on_conflict", "estate_name,room_name,rent,commonfee")])
            .header("Prefer", "resolution=merge-duplicates")
            .json(room);
        self.auth(req)
            .send()
            .await
            .context("Failed to insert vacancy into Supabase")?
            .error_for_status()
            .context("Supabase returned error status on insert")?;

        Ok(())
    }

    /// Delete a vacancy that is no longer listed on UR so it can re-trigger if relisted later
    pub async fn remove_unlisted(&self, room: &RoomListing) -> Result<()> {
        let req = self.client
            .delete(&self.base_url)
            .query(&[
                ("estate_name", format!("eq.{}", room.estate_name)),
                ("room_name", format!("eq.{}", room.room_name)),
                ("rent", format!("eq.{}", room.rent)),
                ("commonfee", format!("eq.{}", room.commonfee)),
            ]);
        self.auth(req)
            .send()
            .await
            .context("Failed to delete stale vacancy from Supabase")?
            .error_for_status()
            .context("Supabase returned error status on delete")?;

        Ok(())
    }
}
