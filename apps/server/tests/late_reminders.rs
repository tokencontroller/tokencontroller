//! Late token sheet reminders against a real Postgres database, with the
//! mail server faked by a local SMTP server.

use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use time::macros::date;
use tokencontroller_core::NoExtension;
use tokencontroller_server::integrations::reminders;
use tokencontroller_server::{AppState, Config};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn config(smtp_url: Option<String>) -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "https://app.example".to_owned(),
        secret_key: "0123456789abcdef0123456789abcdef".to_owned(),
        smtp_url,
        smtp_from: None,
        prices_refresh: false,
        customer_relations: tokencontroller_server::customer_relations::Settings::default(),
        stripe: None,
        embeddings: None,
    }
}

/// A mail server that accepts everything and keeps each message's data.
async fn fake_smtp() -> TestResult<(String, Arc<Mutex<Vec<String>>>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("smtp://{}", listener.local_addr()?);
    let received = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&received);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let keep = Arc::clone(&keep);
            tokio::spawn(async move {
                let _served = serve_smtp(socket, keep).await;
            });
        }
    });
    Ok((url, received))
}

async fn serve_smtp(
    socket: tokio::net::TcpStream,
    keep: Arc<Mutex<Vec<String>>>,
) -> TestResult<()> {
    let (read, mut write) = socket.into_split();
    let mut lines = BufReader::new(read).lines();
    write.write_all(b"220 fake ESMTP\r\n").await?;
    let mut data: Option<String> = None;
    while let Some(line) = lines.next_line().await? {
        if let Some(body) = data.as_mut() {
            // Undo quoted-printable soft line breaks, so tests can read long lines.
            if let Some(soft) = line.strip_suffix('=') {
                body.push_str(soft);
                continue;
            }
            if line == "." {
                keep.lock()
                    .map_err(|_| "poisoned")?
                    .push(data.take().unwrap_or_default());
                write.write_all(b"250 queued\r\n").await?;
            } else {
                body.push_str(&line);
                body.push('\n');
            }
            continue;
        }
        let command = line.to_uppercase();
        if command.starts_with("EHLO") || command.starts_with("HELO") {
            write.write_all(b"250 fake\r\n").await?;
        } else if command.starts_with("DATA") {
            data = Some(String::new());
            write.write_all(b"354 go\r\n").await?;
        } else if command.starts_with("QUIT") {
            write.write_all(b"221 bye\r\n").await?;
            break;
        } else {
            write.write_all(b"250 ok\r\n").await?;
        }
    }
    Ok(())
}

/// Kestrel Digital with a weekly cadence from Monday 28 September 2026.
/// Mia submitted her first week and nothing since; she has two drafts in
/// the week of 5 October. Jonas never submits token sheets.
async fn kestrel(pool: &PgPool) -> TestResult<Uuid> {
    let mut conn = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(
        &mut conn,
        "Kestrel Digital",
        "ana.silva@acme.example",
    )
    .await
    .map_err(|error| error.to_string())?;
    let organization_id = created.organization_id;
    let mia: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, name, joined_at)
         VALUES ($1, 'm.ortner@kestrel.example', 'Mara Ortner', now()) RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "INSERT INTO people (organization_id, email, name, joined_at)
         VALUES ($1, 'jonas@kestrel.example', 'Jonas Weber', now())",
    )
    .bind(organization_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO cadences (organization_id, kind, starts_on, set_by)
         VALUES ($1, 'weekly', '2026-09-28', $2)",
    )
    .bind(organization_id)
    .bind(created.person_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO token_sheets (organization_id, person_id, starts_on, ends_on)
         VALUES ($1, $2, '2026-09-28', '2026-10-04')",
    )
    .bind(organization_id)
    .bind(mia)
    .execute(pool)
    .await?;
    for started in ["2026-10-06T09:00:00Z", "2026-10-08T14:00:00Z"] {
        sqlx::query(
            "INSERT INTO token_entries (organization_id, agent_manager_id, agent_harness, model,
                 provider, started_at, ended_at, status, on_token_sheet, door)
             VALUES ($1, $2, 'claude_code', 'claude-sonnet-4-5', 'anthropic',
                 $3::timestamptz, $3::timestamptz + interval '1 hour', 'draft', true, 'manual')",
        )
        .bind(organization_id)
        .bind(mia)
        .bind(started)
        .execute(pool)
        .await?;
    }
    Ok(organization_id)
}

fn state(pool: &PgPool, smtp_url: Option<String>) -> AppState {
    AppState::new(pool.clone(), config(smtp_url), Arc::new(NoExtension))
}

async fn reminded(pool: &PgPool) -> TestResult<Vec<time::Date>> {
    Ok(
        sqlx::query_scalar("SELECT starts_on FROM late_sheet_reminders ORDER BY starts_on")
            .fetch_all(pool)
            .await?,
    )
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_late_token_sheet_gets_one_reminder(pool: PgPool) {
    kestrel(&pool).await.unwrap();
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = state(&pool, Some(smtp));

    let note = reminders::send_due(&state, date!(2026 - 10 - 13))
        .await
        .unwrap();
    assert_eq!(note, "1 of 1 late token sheet reminders sent");
    let mails = sent.lock().unwrap().clone();
    assert_eq!(mails.len(), 1);
    let mail = mails.first().cloned().unwrap_or_default();
    assert!(mail.contains("To: m.ortner@kestrel.example"), "{mail}");
    assert!(mail.contains("Subject: Your token sheet is late"), "{mail}");
    assert!(mail.contains("5 to 11 October 2026"), "{mail}");
    assert!(
        mail.contains("It holds 2 token entries waiting for your review."),
        "{mail}"
    );
    assert!(mail.contains("https://app.example/review"), "{mail}");
    assert_eq!(reminded(&pool).await.unwrap(), vec![date!(2026 - 10 - 05)]);

    // The same token sheet never gets a second mail.
    let again = reminders::send_due(&state, date!(2026 - 10 - 14))
        .await
        .unwrap();
    assert_eq!(again, "0 of 0 late token sheet reminders sent");
    assert_eq!(sent.lock().unwrap().len(), 1);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_the_newest_recent_late_token_sheet_is_mailed(pool: PgPool) {
    kestrel(&pool).await.unwrap();
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = state(&pool, Some(smtp));

    // Three weeks are late by now; only the last one gets a mail, and it
    // holds no drafts.
    let note = reminders::send_due(&state, date!(2026 - 10 - 30))
        .await
        .unwrap();
    assert_eq!(note, "1 of 1 late token sheet reminders sent");
    assert_eq!(reminded(&pool).await.unwrap(), vec![date!(2026 - 10 - 19)]);
    let mail = sent.lock().unwrap().first().cloned().unwrap_or_default();
    assert!(mail.contains("19 to 25 October 2026"), "{mail}");
    assert!(
        mail.contains("If there was nothing to report, submit it empty."),
        "{mail}"
    );

    // Months later only the week just over gets a mail; the weeks between
    // show in the web app and the Claude Code mod.
    let late = reminders::send_due(&state, date!(2027 - 03 - 01))
        .await
        .unwrap();
    assert_eq!(late, "1 of 1 late token sheet reminders sent");
    assert_eq!(
        reminded(&pool).await.unwrap(),
        vec![date!(2026 - 10 - 19), date!(2027 - 02 - 22)]
    );
    let quiet = reminders::send_due(&state, date!(2027 - 03 - 02))
        .await
        .unwrap();
    assert_eq!(quiet, "0 of 0 late token sheet reminders sent");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn without_a_mail_server_nothing_is_sent_or_kept(pool: PgPool) {
    kestrel(&pool).await.unwrap();

    let note = reminders::send_due(&state(&pool, None), date!(2026 - 10 - 13))
        .await
        .unwrap();
    assert_eq!(
        note,
        "no mail server is set (TC_SMTP_URL), so no reminders were sent"
    );
    assert!(reminded(&pool).await.unwrap().is_empty());

    // A mail server that cannot be reached is an outage, and the reminder
    // waits for the next run.
    let down = reminders::send_due(
        &state(&pool, Some("smtp://127.0.0.1:9".to_owned())),
        date!(2026 - 10 - 13),
    )
    .await;
    let message = down
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.starts_with("the mail server took none of 1 late token sheet reminders"),
        "{message}"
    );
    assert!(reminded(&pool).await.unwrap().is_empty());
}
