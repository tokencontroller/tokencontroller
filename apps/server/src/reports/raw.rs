//! The raw export (docs: Report > Raw export): everything an organization
//! has in Token Controller as a zip of JSON files, so leaving is easy. Each
//! file is a JSON array of rows exactly as stored, children nested in their
//! parent: splits with their tags in their token entry, revisions in their
//! bill, snapshots in their closed period. A `manifest.json` names the
//! files. Secrets, such as the hashes behind shared cost sheet links, are
//! left out.

use std::io::{Cursor, Write};

use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::AppState;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::sign_in::{Caller, Credential};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(raw_export))
}

/// Files with rows that carry their own `organization_id`: the file name,
/// the table, and the row as JSON (`t` is the row). A table that does not
/// exist is left out.
const FLAT: &[(&str, &str, &str)] = &[
    ("people.json", "people", "to_jsonb(t)"),
    ("projects.json", "projects", "to_jsonb(t)"),
    ("tickets.json", "tickets", "to_jsonb(t)"),
    ("tag_names.json", "tag_names", "to_jsonb(t)"),
    ("tag_map_versions.json", "tag_map_versions", "to_jsonb(t)"),
    ("rules.json", "rules", "to_jsonb(t)"),
    ("tag_rules.json", "tag_rules", "to_jsonb(t)"),
    ("adjustments.json", "adjustments", "to_jsonb(t)"),
    ("cadences.json", "cadences", "to_jsonb(t)"),
    ("token_budgets.json", "token_budgets", "to_jsonb(t)"),
    ("token_forecasts.json", "token_forecasts", "to_jsonb(t)"),
    ("saved_reports.json", "saved_reports", "to_jsonb(t)"),
    ("cost_sheet_links.json", "cost_sheet_links", "to_jsonb(t) - 'secret_hash'"),
];

/// Files whose rows nest their children, as one query each taking the
/// organization as `$1`.
const NESTED: &[(&str, &str)] = &[
    (
        "token_entries.json",
        r"SELECT coalesce(jsonb_agg(to_jsonb(e) || jsonb_build_object(
              'splits', coalesce((
                  SELECT jsonb_agg(to_jsonb(s) || jsonb_build_object('tags', coalesce((
                      SELECT jsonb_object_agg(t.name, t.value) FROM split_tags t WHERE t.split_id = s.id
                  ), '{}'::jsonb)) ORDER BY s.id)
                  FROM splits s WHERE s.token_entry_id = e.id
              ), '[]'::jsonb),
              'decisions', coalesce((
                  SELECT jsonb_agg(to_jsonb(d) ORDER BY d.decided_at)
                  FROM token_entry_decisions d WHERE d.token_entry_id = e.id
              ), '[]'::jsonb)
          ) ORDER BY e.started_at, e.id), '[]'::jsonb)
          FROM token_entries e WHERE e.organization_id = $1",
    ),
    (
        "bills.json",
        r"SELECT coalesce(jsonb_agg(to_jsonb(b) || jsonb_build_object(
              'revisions', coalesce((
                  SELECT jsonb_agg(to_jsonb(r) ORDER BY r.revision)
                  FROM bill_revisions r WHERE r.bill_id = b.id
              ), '[]'::jsonb)
          ) ORDER BY b.period, b.provider, b.cost_source), '[]'::jsonb)
          FROM bills b WHERE b.organization_id = $1",
    ),
    (
        "periods.json",
        r"SELECT coalesce(jsonb_agg(to_jsonb(c) || jsonb_build_object(
              'bills', coalesce((
                  SELECT jsonb_agg(to_jsonb(cb) ORDER BY cb.bill_revision_id)
                  FROM closed_period_bills cb
                  WHERE cb.organization_id = c.organization_id AND cb.period = c.period
              ), '[]'::jsonb),
              'exchange_rates', coalesce((
                  SELECT jsonb_agg(to_jsonb(cr) ORDER BY cr.currency)
                  FROM closed_period_rates cr
                  WHERE cr.organization_id = c.organization_id AND cr.period = c.period
              ), '[]'::jsonb)
          ) ORDER BY c.period), '[]'::jsonb)
          FROM closed_periods c WHERE c.organization_id = $1",
    ),
    (
        "token_sheets.json",
        r"SELECT coalesce(jsonb_agg(to_jsonb(ts) || jsonb_build_object(
              'token_entry_ids', coalesce((
                  SELECT jsonb_agg(se.token_entry_id ORDER BY se.token_entry_id)
                  FROM token_sheet_entries se WHERE se.token_sheet_id = ts.id
              ), '[]'::jsonb)
          ) ORDER BY ts.starts_on, ts.person_id), '[]'::jsonb)
          FROM token_sheets ts WHERE ts.organization_id = $1",
    ),
];

/// Whether `table` exists and has an `organization_id` column.
async fn has_organization_rows(db: &PgPool, table: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM information_schema.columns
               WHERE table_schema = current_schema() AND table_name = $1 AND column_name = 'organization_id'
           ) AS "exists!""#,
        table,
    )
    .fetch_one(db)
    .await
}

/// Every file of the export with its JSON, in a fixed order.
async fn collect(db: &PgPool, organization_id: Uuid) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let mut files = Vec::new();
    for (name, query) in NESTED {
        let rows: Value = sqlx::query_scalar(*query).bind(organization_id).fetch_one(db).await?;
        files.push((*name, rows));
    }
    for (name, table, row) in FLAT {
        if !has_organization_rows(db, table).await? {
            continue;
        }
        // The table and row expressions are the constants above, never
        // input.
        let query = format!(
            "SELECT coalesce(jsonb_agg({row}), '[]'::jsonb) FROM {table} t WHERE t.organization_id = $1"
        );
        let rows: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(query)).bind(organization_id).fetch_one(db).await?;
        files.push((*name, rows));
    }
    files.sort_by_key(|(name, _)| *name);
    Ok(files)
}

/// The zip of `files` with a manifest first.
fn zip(organization_id: Uuid, exported_at: OffsetDateTime, files: &[(&str, Value)]) -> anyhow::Result<Vec<u8>> {
    let manifest = json!({
        "organization_id": organization_id,
        "exported_at": exported_at.format(&time::format_description::well_known::Rfc3339)?,
        "service_version": env!("CARGO_PKG_VERSION"),
        "files": files
            .iter()
            .map(|(name, rows)| json!({
                "name": name,
                "rows": rows.as_array().map_or(0, Vec::len),
            }))
            .collect::<Vec<_>>(),
    });
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("manifest.json", options)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for (name, rows) in files {
        writer.start_file(*name, options)?;
        writer.write_all(&serde_json::to_vec_pretty(rows)?)?;
    }
    Ok(writer.finish()?.into_inner())
}

/// Everything the organization has, as a zip of JSON files. Controllers
/// only.
#[utoipa::path(
    get,
    path = "/exports/raw",
    tag = "reports",
    responses(
        (status = OK, description = "A zip of JSON files", content_type = "application/zip", body = Vec<u8>),
        (status = FORBIDDEN, description = "Only controllers export everything", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn raw_export(State(state): State<AppState>, caller: Caller) -> Result<Response, ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_credential(&[Credential::Session])?;
    let exported_at = OffsetDateTime::now_utc();
    let files = collect(&state.db, caller.organization_id).await?;
    let bytes = zip(caller.organization_id, exported_at, &files).map_err(ApiError::Internal)?;
    let filename = format!("attachment; filename=\"tokencontroller-raw-export-{}.zip\"", exported_at.date());
    let disposition = HeaderValue::from_str(&filename).unwrap_or_else(|_| HeaderValue::from_static("attachment"));
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("application/zip")),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn the_zip_holds_a_manifest_and_every_file() {
        let organization = Uuid::new_v4();
        let files = [
            ("bills.json", json!([{"id": 1}])),
            ("token_entries.json", json!([{"id": 2}, {"id": 3}])),
        ];
        let bytes = zip(organization, OffsetDateTime::UNIX_EPOCH, &files).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 3);
        let mut text = String::new();
        archive.by_name("manifest.json").unwrap().read_to_string(&mut text).unwrap();
        let manifest: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest["organization_id"], json!(organization));
        assert_eq!(manifest["files"][1], json!({"name": "token_entries.json", "rows": 2}));
        let mut entries = String::new();
        archive.by_name("token_entries.json").unwrap().read_to_string(&mut entries).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&entries).unwrap(), json!([{"id": 2}, {"id": 3}]));
    }

    #[test]
    fn flat_files_name_known_tables_only() {
        for (name, table, row) in FLAT {
            assert_eq!(std::path::Path::new(name).extension(), Some("json".as_ref()));
            assert!(table.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert!(row.starts_with("to_jsonb(t)"));
        }
    }
}
