//! Submit reminders against a real Postgres database, with the mail server
//! faked by a local SMTP server: an org manager asks everyone to submit by
//! a day, and each member who has not is mailed once.

use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use time::macros::{date, datetime};
use tokencontroller_core::{DateRange, Event, NoExtension, Record, Submit, TagValue};
use tokencontroller_server::submissions;
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
        price_table: None,
        rates_refresh: false,
        classifier: None,
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

/// Kestrel Digital: Ana (the org manager), Mara and Jonas. Ana asks
/// everyone to submit by the end of 2099, so the request stays open.
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
    for (email, name) in [
        ("m.ortner@kestrel.example", "Mara Ortner"),
        ("jonas@kestrel.example", "Jonas Weber"),
    ] {
        sqlx::query(
            "INSERT INTO people (organization_id, email, name, joined_at) VALUES ($1, $2, $3, now())",
        )
        .bind(organization_id)
        .bind(email)
        .bind(name)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO submit_requests (organization_id, requested_by, until_day, message)
         VALUES ($1, $2, '2099-12-31', 'We close the year.')",
    )
    .bind(organization_id)
    .bind(created.person_id)
    .execute(pool)
    .await?;
    Ok(organization_id)
}

fn state(pool: &PgPool, smtp_url: Option<String>) -> AppState {
    AppState::new(pool.clone(), config(smtp_url), Arc::new(NoExtension))
}

/// Who was mailed, by email.
async fn reminded(pool: &PgPool) -> TestResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT p.email FROM submit_reminders r JOIN people p ON p.id = r.person_id ORDER BY p.email",
    )
    .fetch_all(pool)
    .await?)
}

/// Mara submits everything through the request's day.
async fn mara_submits(pool: &PgPool, organization_id: Uuid) -> TestResult<()> {
    let event = Event::new(
        "sub_test_mara",
        datetime!(2026-10-01 09:00 UTC),
        Record::Submit(Submit {
            person: TagValue::new("m.ortner@kestrel.example")?,
            window: DateRange::new(date!(2026 - 01 - 01), date!(2099 - 12 - 31))?,
        }),
    )?;
    sqlx::query(
        "INSERT INTO events (organization_id, id, recorded_at, kind, payload) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(organization_id)
    .bind(event.id.to_string())
    .bind(event.recorded_at)
    .bind(event.record.kind())
    .bind(serde_json::to_value(&event.record)?)
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn everyone_who_has_not_submitted_gets_one_reminder(pool: PgPool) {
    let organization_id = kestrel(&pool).await.unwrap();
    mara_submits(&pool, organization_id).await.unwrap();
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = state(&pool, Some(smtp));

    let count = submissions::send_reminders(&state).await.unwrap();
    assert_eq!(count, 2, "Ana and Jonas; Mara has submitted");
    let mails = sent.lock().unwrap().clone();
    assert_eq!(mails.len(), 2);
    let jonas = mails
        .iter()
        .find(|mail| mail.contains("To: jonas@kestrel.example"))
        .cloned()
        .unwrap_or_default();
    assert!(
        jonas.contains("Subject: Please submit your sessions by 2099-12-31"),
        "{jonas}"
    );
    assert!(jonas.contains("Hello Jonas Weber"), "{jonas}");
    assert!(
        jonas.contains("asks everyone at Kestrel Digital"),
        "{jonas}"
    );
    assert!(jonas.contains("We close the year."), "{jonas}");
    assert!(jonas.contains("app.example"), "{jonas}");
    assert!(!jonas.contains("controller"), "{jonas}");
    assert_eq!(
        reminded(&pool).await.unwrap(),
        vec![
            "ana.silva@acme.example".to_owned(),
            "jonas@kestrel.example".to_owned(),
            "m.ortner@kestrel.example".to_owned(),
        ],
        "a member who has submitted is marked done without a mail"
    );

    // Nobody gets a second mail for the same request.
    let again = submissions::send_reminders(&state).await.unwrap();
    assert_eq!(again, 0);
    assert_eq!(sent.lock().unwrap().len(), 2);

    // The job reports what it did.
    let note = submissions::run_job(state.clone()).await.unwrap();
    assert_eq!(note, "0 reminders sent");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_request_whose_day_has_passed_mails_nobody(pool: PgPool) {
    let organization_id = kestrel(&pool).await.unwrap();
    sqlx::query("UPDATE submit_requests SET until_day = '2020-01-31' WHERE organization_id = $1")
        .bind(organization_id)
        .execute(&pool)
        .await
        .unwrap();
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = state(&pool, Some(smtp));

    let count = submissions::send_reminders(&state).await.unwrap();
    assert_eq!(count, 0);
    assert!(sent.lock().unwrap().is_empty());
    assert!(reminded(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn without_a_mail_server_nothing_is_sent_or_kept(pool: PgPool) {
    kestrel(&pool).await.unwrap();

    let count = submissions::send_reminders(&state(&pool, None))
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        reminded(&pool).await.unwrap().is_empty(),
        "the reminders wait for a mail server"
    );

    // A mail server that cannot be reached: the reminders wait for the
    // next run, and nothing is kept.
    let count = submissions::send_reminders(&state(&pool, Some("smtp://127.0.0.1:9".to_owned())))
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(reminded(&pool).await.unwrap().is_empty());
}
