//! The support inbox, the site assistant and the newsletter against a real
//! Postgres database, with the Claude API, Resend and the docs site faked by
//! a local HTTP server and the mail server by a local SMTP server.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use hmac::{KeyInit as _, Mac as _};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgPool, Row as _};
use tokencontroller_core::NoExtension;
use tokencontroller_server::customer_relations::{self, Settings};
use tokencontroller_server::sign_in::secret;
use tokencontroller_server::{AppState, Config, router};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tower::ServiceExt as _;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const SECRET: &str = "inbound-secret";
const SITE: &str = "https://site.example";

fn config(settings: Settings, smtp_url: Option<String>) -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "https://app.example".to_owned(),
        secret_key: "0123456789abcdef0123456789abcdef".to_owned(),
        smtp_url,
        smtp_from: None,
        prices_refresh: false,
        customer_relations: settings,
        stripe: None,
    }
}

fn settings(fake: &MockServer) -> Settings {
    Settings {
        operator_emails: vec!["ops@example.com".to_owned()],
        inbound_mail_secret: Some(SECRET.to_owned()),
        support_from: Some("Token Controller support <support@example.com>".to_owned()),
        anthropic_api_key: Some("sk-ant-test".to_owned()),
        resend_api_key: Some("re_test".to_owned()),
        newsletter_from: Some("Token Controller <news@news.example.com>".to_owned()),
        site_url: Some(SITE.to_owned()),
        anthropic_api_url: fake.uri(),
        resend_api_url: fake.uri(),
        docs_text_url: format!("{}/llms-full.txt", fake.uri()),
    }
}

/// A fake docs site, Claude API and Resend API in one server.
async fn fake_services(claude_answer: Value) -> MockServer {
    let fake = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/llms-full.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("# Plans\n\nURL: https://docs.example/plans\n\nSolo is free."),
        )
        .mount(&fake)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-ant-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{ "type": "text", "text": claude_answer.to_string() }],
            "stop_reason": "end_turn"
        })))
        .mount(&fake)
        .await;
    Mock::given(method("POST"))
        .and(path("/emails"))
        .and(header("authorization", "Bearer re_test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "sent" })))
        .mount(&fake)
        .await;
    fake
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

fn signature(id: &str, body: &[u8]) -> TestResult<String> {
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(SECRET.as_bytes())?;
    mac.update(id.as_bytes());
    mac.update(b"\n");
    mac.update(body);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

async fn hand_over(state: &AppState, id: &str, mailbox: &str, raw: &str) -> TestResult<StatusCode> {
    let request = Request::post("/api/v1/inbound/mail")
        .header("content-type", "message/rfc822")
        .header("x-tc-message-id", id)
        .header("x-tc-mailbox", mailbox)
        .header("x-tc-envelope-from", "ana@kestrel.example")
        .header("x-tc-envelope-to", format!("{mailbox}@example.com"))
        .header("x-tc-signature", signature(id, raw.as_bytes())?)
        .body(Body::from(raw.to_owned()))?;
    Ok(router(state.clone()).oneshot(request).await?.status())
}

async fn post_json(
    state: &AppState,
    uri: &str,
    body: &Value,
) -> TestResult<(StatusCode, Value, Option<String>)> {
    let request = Request::post(uri)
        .header("content-type", "application/json")
        .header("origin", SITE)
        .body(Body::from(body.to_string()))?;
    let response = router(state.clone()).oneshot(request).await?;
    let status = response.status();
    let origin = response
        .headers()
        .get("access-control-allow-origin")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = response.into_body().collect().await?.to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, json, origin))
}

const QUESTION: &str = "From: Ana Ortner <ana@kestrel.example>\r\n\
To: support@example.com\r\n\
Subject: Is Solo free?\r\n\
Message-ID: <q1@kestrel.example>\r\n\
\r\n\
Hi, is Solo really free?\r\n";

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_question_by_mail_is_answered_from_the_docs(pool: PgPool) {
    let fake = fake_services(json!({
        "action": "answer", "kind": "question",
        "answer": "Hi Ana, yes. Solo is free.", "links": ["https://docs.example/plans"], "reason": ""
    }))
    .await;
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), Some(smtp)),
        Arc::new(NoExtension),
    );

    assert_eq!(
        hand_over(&state, "m1", "support", QUESTION).await.unwrap(),
        StatusCode::NO_CONTENT
    );
    // The worker may hand the same mail over twice.
    assert_eq!(
        hand_over(&state, "m1", "support", QUESTION).await.unwrap(),
        StatusCode::NO_CONTENT
    );

    customer_relations::run_once(&state).await;

    let conversation =
        sqlx::query("SELECT state, kind, contact_email, contact_name FROM support_conversations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(conversation.get::<String, _>("state"), "answered");
    assert_eq!(conversation.get::<String, _>("kind"), "question");
    assert_eq!(
        conversation.get::<String, _>("contact_email"),
        "ana@kestrel.example"
    );
    assert_eq!(
        conversation
            .get::<Option<String>, _>("contact_name")
            .as_deref(),
        Some("Ana Ortner")
    );

    let answer = sqlx::query("SELECT body, links, sent_at IS NOT NULL AS sent FROM support_messages WHERE author = 'assistant'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let body: String = answer.get("body");
    assert!(body.starts_with("Hi Ana, yes. Solo is free."));
    assert!(body.contains("If this did not help, reply and Jan will answer."));
    assert!(body.contains("written by an AI assistant"));
    assert_eq!(
        answer.get::<Vec<String>, _>("links"),
        ["https://docs.example/plans"]
    );
    assert!(answer.get::<bool, _>("sent"));

    let mails = sent.lock().unwrap().clone();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].contains("Subject: Re: Is Solo free?"));
    assert!(mails[0].contains("In-Reply-To: <q1@kestrel.example>"));
    assert!(mails[0].contains("To: \"Ana Ortner\" <ana@kestrel.example>"));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_reply_reopens_its_conversation(pool: PgPool) {
    let fake = fake_services(json!({
        "action": "answer", "kind": "question", "answer": "Yes.", "links": [], "reason": ""
    }))
    .await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    hand_over(&state, "m1", "support", QUESTION).await.unwrap();
    customer_relations::run_once(&state).await;
    let ours: String = sqlx::query_scalar(
        "SELECT mail_message_id FROM support_messages WHERE author = 'assistant'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let reply = format!(
        "From: ana@kestrel.example\r\nSubject: Re: Is Solo free?\r\nMessage-ID: <q2@kestrel.example>\r\nIn-Reply-To: <{ours}>\r\nReferences: <q1@kestrel.example> <{ours}>\r\n\r\nAnd the Team plan?\r\n"
    );
    hand_over(&state, "m2", "support", &reply).await.unwrap();

    let conversations: i64 = sqlx::query_scalar("SELECT count(*) FROM support_conversations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(conversations, 1);
    let state_now: String = sqlx::query_scalar("SELECT state FROM support_conversations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state_now, "new");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn what_the_docs_cannot_answer_goes_to_the_operators(pool: PgPool) {
    let fake = fake_services(json!({
        "action": "escalate", "kind": "sales",
        "answer": "Proposed: book a call.", "links": [], "reason": "Asks for a call"
    }))
    .await;
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), Some(smtp)),
        Arc::new(NoExtension),
    );
    hand_over(&state, "m1", "support", QUESTION).await.unwrap();
    customer_relations::run_once(&state).await;

    let row = sqlx::query("SELECT id, state, kind, suggested_answer, escalation_reason, notified_at IS NOT NULL AS notified FROM support_conversations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("state"), "escalated");
    assert_eq!(row.get::<String, _>("kind"), "sales");
    assert_eq!(
        row.get::<Option<String>, _>("suggested_answer").as_deref(),
        Some("Proposed: book a call.")
    );
    assert_eq!(
        row.get::<Option<String>, _>("escalation_reason").as_deref(),
        Some("Asks for a call")
    );
    assert!(row.get::<bool, _>("notified"));
    let id: sqlx::types::Uuid = row.get("id");

    let mails = sent.lock().unwrap().clone();
    assert_eq!(
        mails.len(),
        2,
        "the person is told Jan answers, and the operator gets a notice"
    );
    let notice = mails
        .iter()
        .find(|mail| mail.contains("To: ops@example.com"))
        .unwrap();
    assert!(notice.contains(&format!("https://app.example/admin/support/{id}")));
    assert!(
        !notice.contains("Solo"),
        "a notice never carries the message"
    );
    let to_person = mails
        .iter()
        .find(|mail| mail.contains("ana@kestrel.example"))
        .unwrap();
    assert!(to_person.contains("Jan will answer you here"));
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn without_a_claude_key_every_question_goes_to_the_operators(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let mut settings = settings(&fake);
    settings.anthropic_api_key = None;
    let state = AppState::new(pool.clone(), config(settings, None), Arc::new(NoExtension));
    hand_over(&state, "m1", "support", QUESTION).await.unwrap();
    customer_relations::run_once(&state).await;
    let state_now: String = sqlx::query_scalar("SELECT state FROM support_conversations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state_now, "escalated");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn auto_replies_are_kept_but_never_answered(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    let auto = "From: ana@kestrel.example\r\nAuto-Submitted: auto-replied\r\nSubject: Out of office\r\n\r\nAway.\r\n";
    hand_over(&state, "m1", "support", auto).await.unwrap();
    customer_relations::run_once(&state).await;
    let row = sqlx::query("SELECT state, kind FROM support_conversations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("state"), "closed");
    assert_eq!(row.get::<String, _>("kind"), "automatic");
    let ours: i64 =
        sqlx::query_scalar("SELECT count(*) FROM support_messages WHERE author <> 'customer'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ours, 0);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn handed_over_mail_must_be_signed(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    let request = Request::post("/api/v1/inbound/mail")
        .header("x-tc-message-id", "m1")
        .header("x-tc-mailbox", "support")
        .header("x-tc-envelope-to", "support@example.com")
        .header("x-tc-signature", "00")
        .body(Body::from(QUESTION))
        .unwrap();
    let status = router(state.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let off = AppState::new(
        pool,
        config(Settings::default(), None),
        Arc::new(NoExtension),
    );
    assert_eq!(
        hand_over(&off, "m1", "support", QUESTION).await.unwrap(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn mail_with_screenshots_fits(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(pool, config(settings(&fake), None), Arc::new(NoExtension));
    let big = format!("{QUESTION}{}", "x".repeat(10 * 1024 * 1024));
    assert_eq!(
        hand_over(&state, "m1", "support", &big).await.unwrap(),
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_contact_form_starts_a_conversation(pool: PgPool) {
    let fake = fake_services(json!({
        "action": "answer", "kind": "question", "answer": "Yes.", "links": [], "reason": ""
    }))
    .await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    let (status, _, origin) = post_json(
        &state,
        "/api/v1/contact",
        &json!({ "name": "Ben", "email": "ben@kestrel.example", "message": "Is Solo free?" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(origin.as_deref(), Some(SITE));

    // A robot fills the hidden field: accepted, and nothing kept.
    let (status, _, _) = post_json(
        &state,
        "/api/v1/contact",
        &json!({ "email": "bot@spam.example", "message": "Buy", "website": "https://spam.example" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);

    let rows = sqlx::query("SELECT channel, contact_email FROM support_conversations")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String, _>("channel"), "contact_form");

    customer_relations::run_once(&state).await;
    let state_now: String = sqlx::query_scalar("SELECT state FROM support_conversations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state_now, "answered");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_site_assistant_answers_and_keeps_the_question(pool: PgPool) {
    let fake = fake_services(json!({
        "answered": true, "answer": "Yes, Solo is free.", "links": ["https://docs.example/plans"]
    }))
    .await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    let (status, body, origin) = post_json(
        &state,
        "/api/v1/assistant/questions",
        &json!({ "question": "Is Solo free?", "page": "/" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["answer"], "Yes, Solo is free.");
    assert_eq!(body["answered"], true);
    assert_eq!(origin.as_deref(), Some(SITE));

    let kept = sqlx::query("SELECT question, page, answered FROM assistant_questions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept.get::<String, _>("question"), "Is Solo free?");
    assert_eq!(kept.get::<Option<String>, _>("page").as_deref(), Some("/"));
    assert!(kept.get::<bool, _>("answered"));

    let (status, _, _) = post_json(
        &state,
        "/api/v1/assistant/questions",
        &json!({ "question": "x".repeat(501) }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let off = AppState::new(
        pool,
        config(Settings::default(), None),
        Arc::new(NoExtension),
    );
    let (status, _, _) = post_json(
        &off,
        "/api/v1/assistant/questions",
        &json!({ "question": "Hi" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_newsletter_needs_a_confirmed_sign_up(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );

    let (status, body, _) = post_json(
        &state,
        "/api/v1/newsletter/subscribers",
        &json!({ "email": "Ana@Kestrel.example", "source": "/blog" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["status"], "check_your_email");
    customer_relations::run_once(&state).await;

    let row = sqlx::query("SELECT status, confirm_token, unsubscribe_token, confirmation_sent_at IS NOT NULL AS sent FROM newsletter_subscribers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert!(
        row.get::<bool, _>("sent"),
        "the confirmation mail went out through Resend"
    );
    let confirm: String = row.get("confirm_token");
    let unsubscribe: String = row.get("unsubscribe_token");

    let resend: Vec<Value> = fake
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/emails")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    assert_eq!(resend.len(), 1);
    assert_eq!(resend[0]["to"][0], "ana@kestrel.example");
    assert!(
        resend[0]["text"]
            .as_str()
            .unwrap()
            .contains(&format!("{SITE}/newsletter/confirm?token={confirm}"))
    );
    assert_eq!(
        resend[0]["headers"]["List-Unsubscribe-Post"],
        "List-Unsubscribe=One-Click"
    );
    assert_eq!(resend[0]["reply_to"], "support@example.com");

    // Signing up again does not send a second mail right away.
    post_json(
        &state,
        "/api/v1/newsletter/subscribers",
        &json!({ "email": "ana@kestrel.example" }),
    )
    .await
    .unwrap();
    customer_relations::run_once(&state).await;
    let count = fake
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/emails")
        .count();
    assert_eq!(count, 1);

    let (status, _, _) = post_json(
        &state,
        "/api/v1/newsletter/confirm",
        &json!({ "token": "wrong" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body, _) = post_json(
        &state,
        "/api/v1/newsletter/confirm",
        &json!({ "token": confirm }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "confirmed");

    // One-click unsubscribe as a mail client sends it (RFC 8058).
    let request = Request::post(format!(
        "/api/v1/newsletter/unsubscribe?token={unsubscribe}"
    ))
    .header("content-type", "application/x-www-form-urlencoded")
    .body(Body::from("List-Unsubscribe=One-Click"))
    .unwrap();
    let status = router(state.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::OK);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM newsletter_subscribers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0, "unsubscribing deletes the address");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn an_issue_goes_to_confirmed_subscribers_only(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    sqlx::query(
        "INSERT INTO newsletter_subscribers (email, source, status, confirm_token, unsubscribe_token, confirmed_at)
         VALUES ('a@kestrel.example', 'test', 'confirmed', 'c1', 'u1', now()),
                ('b@kestrel.example', 'test', 'pending', 'c2', 'u2', NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE newsletter_subscribers SET confirmation_sent_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let issue: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO newsletter_issues (subject, body, status, send_started_at) VALUES ('News', 'Version 1.0.0.', 'sending', now()) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO newsletter_deliveries (issue_id, subscriber_id) SELECT $1, id FROM newsletter_subscribers WHERE status = 'confirmed'",
    )
    .bind(issue)
    .execute(&pool)
    .await
    .unwrap();

    customer_relations::run_once(&state).await;

    let status: String = sqlx::query_scalar("SELECT status FROM newsletter_issues")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "sent");
    let sent: Vec<Value> = fake
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/emails")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["to"][0], "a@kestrel.example");
    assert_eq!(sent[0]["from"], "Token Controller <news@news.example.com>");
    assert!(
        sent[0]["text"]
            .as_str()
            .unwrap()
            .contains(&format!("{SITE}/newsletter/unsubscribe?token=u1"))
    );
    assert_eq!(
        sent[0]["headers"]["List-Unsubscribe"],
        "<https://app.example/api/v1/newsletter/unsubscribe?token=u1>"
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn old_questions_mail_and_conversations_are_deleted(pool: PgPool) {
    // Retention runs with every feature off too.
    let state = AppState::new(
        pool.clone(),
        config(Settings::default(), None),
        Arc::new(NoExtension),
    );
    sqlx::query(
        "INSERT INTO assistant_questions (question, answer, answered, created_at) VALUES
         ('old', 'a', true, now() - interval '91 days'), ('new', 'a', true, now() - interval '89 days')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO inbound_mail (message_id, mailbox, envelope_from, envelope_to, raw, received_at) VALUES
         ('old', 'support', 'a@example.com', 'support@example.com', 'x', now() - interval '91 days'),
         ('new', 'support', 'a@example.com', 'support@example.com', 'x', now() - interval '89 days')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Both started three years ago; only the second had a message last month.
    for (subject, last_message) in [("quiet", "3 years"), ("ongoing", "1 month")] {
        sqlx::query(
            "WITH c AS (
               INSERT INTO support_conversations (channel, contact_email, subject, created_at)
               VALUES ('mail', 'a@example.com', $1, now() - interval '3 years') RETURNING id
             )
             INSERT INTO support_messages (conversation_id, author, body, inbound_message_id, created_at)
             SELECT id, 'customer', 'Hello', 'old', now() - $2::interval FROM c",
        )
        .bind(subject)
        .bind(last_message)
        .execute(&pool)
        .await
        .unwrap();
    }

    customer_relations::run_once(&state).await;

    let questions: Vec<String> = sqlx::query_scalar("SELECT question FROM assistant_questions")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(questions, ["new"]);
    let mail: Vec<String> = sqlx::query_scalar("SELECT message_id FROM inbound_mail")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(mail, ["new"]);
    let conversations: Vec<String> =
        sqlx::query_scalar("SELECT subject FROM support_conversations")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(conversations, ["ongoing"]);
    // The message keeps its text when its raw mail is gone.
    let body: String =
        sqlx::query_scalar("SELECT body FROM support_messages WHERE inbound_message_id IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(body, "Hello");
}

// --- The admin area and feedback reports ---

/// A new organization with a signed-in controller: its id, the person and
/// their session token.
async fn organization(pool: &PgPool, name: &str, email: &str) -> TestResult<(Uuid, Uuid, String)> {
    let mut connection = pool.acquire().await?;
    let created = tokencontroller_server::organizations::create(&mut connection, name, email)
        .await
        .map_err(|error| error.to_string())?;
    let session = session_for(pool, created.person_id).await?;
    Ok((created.organization_id, created.person_id, session))
}

/// A joined member of `organization` with `role`, and their session token.
async fn member(
    pool: &PgPool,
    organization: Uuid,
    email: &str,
    role: &str,
) -> TestResult<(Uuid, String)> {
    let person: Uuid = sqlx::query_scalar(
        "INSERT INTO people (organization_id, email, role, joined_at) VALUES ($1, $2, $3, now()) RETURNING id",
    )
    .bind(organization)
    .bind(email)
    .bind(role)
    .fetch_one(pool)
    .await?;
    Ok((person, session_for(pool, person).await?))
}

async fn session_for(pool: &PgPool, person_id: Uuid) -> TestResult<String> {
    let session = secret::generate(secret::SESSION, "0123456789abcdef0123456789abcdef")?;
    sqlx::query("INSERT INTO sessions (token_hash, person_id, client, expires_at) VALUES ($1, $2, 'browser', now() + interval '1 day')")
        .bind(&session.hash)
        .bind(person_id)
        .execute(pool)
        .await?;
    Ok(session.token)
}

/// Calls the API as the person with `session`; returns the status, the JSON
/// body (or Null) and the content type.
async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    session: Option<&str>,
    body: Option<Value>,
) -> TestResult<(StatusCode, Value, String)> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(session) = session {
        request = request.header("cookie", format!("tc_session={session}"));
    }
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))?,
        None => request.body(Body::empty())?,
    };
    let response = router(state.clone()).oneshot(request).await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.into_body().collect().await?.to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok((status, json, content_type))
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really an image";

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn only_operators_see_the_admin_area(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let (smtp, sent) = fake_smtp().await.unwrap();
    let mut settings = settings(&fake);
    settings.anthropic_api_key = None;
    let state = AppState::new(
        pool.clone(),
        config(settings, Some(smtp)),
        Arc::new(NoExtension),
    );
    let (_, _, ops) = organization(&pool, "Token Controller", "ops@example.com")
        .await
        .unwrap();
    let (kestrel, _, ana) = organization(&pool, "Kestrel Digital", "ana@kestrel.example")
        .await
        .unwrap();

    let (status, overview, _) = call(&state, "GET", "/api/v1/admin", Some(&ops), None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview["support_escalated"], 0);
    for session in [Some(ana.as_str()), None] {
        for uri in [
            "/api/v1/admin",
            "/api/v1/admin/support",
            "/api/v1/admin/customers",
            "/api/v1/admin/newsletter",
        ] {
            let (status, _, _) = call(&state, "GET", uri, session, None).await.unwrap();
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri} is hidden");
        }
    }

    // Without a Claude key the contact form message waits for a person.
    let (status, _, _) = post_json(
        &state,
        "/api/v1/contact",
        &json!({"email": "ana@kestrel.example", "message": "Can we pay by invoice?", "website": ""}),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    customer_relations::run_once(&state).await;
    let (_, list, _) = call(&state, "GET", "/api/v1/admin/support", Some(&ops), None)
        .await
        .unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["state"], "escalated");
    let id = list[0]["id"].as_str().unwrap().to_owned();
    let (_, conversation, _) = call(
        &state,
        "GET",
        &format!("/api/v1/admin/support/{id}"),
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        conversation["messages"][0]["body"],
        "Can we pay by invoice?"
    );
    assert_eq!(conversation["customer"]["id"], kestrel.to_string());

    let (status, _, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/support/{id}/replies"),
        Some(&ana),
        Some(json!({"text": "Hi"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND, "only operators answer");
    let (status, _, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/support/{id}/replies"),
        Some(&ops),
        Some(json!({"text": " "})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, answered, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/support/{id}/replies"),
        Some(&ops),
        Some(json!({"text": "Hi Ana, yes. Write when you are ready."})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(answered["conversation"]["state"], "answered");
    customer_relations::run_once(&state).await;
    let mails = sent.lock().unwrap().clone();
    let reply = mails
        .iter()
        .find(|mail| mail.contains("Write when you are ready"))
        .unwrap();
    assert!(reply.contains("ana@kestrel.example"));
    assert!(reply.contains("Token Controller support"));

    let (status, closed, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/support/{id}/close"),
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["conversation"]["state"], "closed");
    let (_, open, _) = call(&state, "GET", "/api/v1/admin/support", Some(&ops), None)
        .await
        .unwrap();
    assert_eq!(open, json!([]));
    let (_, all_closed, _) = call(
        &state,
        "GET",
        "/api/v1/admin/support?state=closed",
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(all_closed.as_array().unwrap().len(), 1);
    let (status, _, _) = call(
        &state,
        "GET",
        "/api/v1/admin/support?state=lost",
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn operators_write_and_send_the_newsletter(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    let (_, _, ops) = organization(&pool, "Token Controller", "ops@example.com")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO newsletter_subscribers (email, source, status, confirm_token, unsubscribe_token, confirmed_at, confirmation_sent_at)
         VALUES ('a@kestrel.example', 'test', 'confirmed', 'c1', 'u1', now(), now()),
                ('b@kestrel.example', 'test', 'pending', 'c2', 'u2', NULL, now())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let (status, draft, _) = call(
        &state,
        "POST",
        "/api/v1/admin/newsletter/issues",
        Some(&ops),
        Some(json!({"subject": "News", "body": "Draft"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(draft["status"], "draft");
    let id = draft["id"].as_str().unwrap().to_owned();
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/admin/newsletter/issues",
        Some(&ops),
        Some(json!({"subject": "Two\nlines", "body": "x"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, changed, _) = call(
        &state,
        "PATCH",
        &format!("/api/v1/admin/newsletter/issues/{id}"),
        Some(&ops),
        Some(json!({"body": "Token Controller 1.0.0 is out."})),
    )
    .await
    .unwrap();
    assert_eq!(changed["subject"], "News");
    assert_eq!(changed["body"], "Token Controller 1.0.0 is out.");

    let (status, sending, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/newsletter/issues/{id}/send"),
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sending["status"], "sending");
    assert_eq!(sending["recipients"], 1, "confirmed subscribers only");
    let (status, _, _) = call(
        &state,
        "PATCH",
        &format!("/api/v1/admin/newsletter/issues/{id}"),
        Some(&ops),
        Some(json!({"body": "late"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/newsletter/issues/{id}/send"),
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "an issue goes out once");

    customer_relations::run_once(&state).await;
    let (_, newsletter, _) = call(&state, "GET", "/api/v1/admin/newsletter", Some(&ops), None)
        .await
        .unwrap();
    assert_eq!(newsletter["on"], true);
    assert_eq!(newsletter["confirmed"], 1);
    assert_eq!(newsletter["pending"], 1);
    assert_eq!(newsletter["issues"][0]["status"], "sent");
    assert_eq!(newsletter["issues"][0]["delivered"], 1);

    // Without Resend nothing can be sent.
    let off = AppState::new(
        pool.clone(),
        config(
            Settings {
                operator_emails: vec!["ops@example.com".to_owned()],
                ..Settings::default()
            },
            None,
        ),
        Arc::new(NoExtension),
    );
    let (_, draft, _) = call(
        &off,
        "POST",
        "/api/v1/admin/newsletter/issues",
        Some(&ops),
        Some(json!({"subject": "Later", "body": "x"})),
    )
    .await
    .unwrap();
    let (status, _, _) = call(
        &off,
        "POST",
        &format!(
            "/api/v1/admin/newsletter/issues/{}/send",
            draft["id"].as_str().unwrap()
        ),
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn feedback_reaches_the_operators_and_the_answer_reaches_the_sender(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), Some(smtp)),
        Arc::new(NoExtension),
    );
    let (_, _, ops) = organization(&pool, "Token Controller", "ops@example.com")
        .await
        .unwrap();
    let (kestrel, _, ana) = organization(&pool, "Kestrel Digital", "ana@kestrel.example")
        .await
        .unwrap();
    let (_, ben) = member(&pool, kestrel, "ben@kestrel.example", "agent_manager")
        .await
        .unwrap();

    let report = json!({
        "kind": "bug",
        "text": "The cost sheet shows the wrong month.",
        "screenshot": {"type": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(PNG)},
        "diagnostics": {
            "page": "/reports/cost-sheet?client=acme",
            "app_version": "1.0.0",
            "browser": "Firefox",
            "screen": "1440x900",
            "errors": ["Could not load the cost sheet."],
            "request_ids": ["req-1"]
        }
    });
    let mut smuggled = report.clone();
    smuggled["diagnostics"]["request_body"] = json!("{\"prompt\": \"secret\"}");
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/feedback",
        Some(&ana),
        Some(smuggled),
    )
    .await
    .unwrap();
    assert!(
        status.is_client_error(),
        "nothing but the listed diagnostics"
    );
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/feedback",
        None,
        Some(report.clone()),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, created, _) = call(&state, "POST", "/api/v1/feedback", Some(&ana), Some(report))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["status"], "received");
    assert_eq!(created["has_screenshot"], true);
    assert_eq!(created["diagnostics"]["page"], "/reports/cost-sheet");
    let id = created["id"].as_str().unwrap().to_owned();

    let (_, mine, _) = call(&state, "GET", "/api/v1/feedback", Some(&ana), None)
        .await
        .unwrap();
    assert_eq!(mine.as_array().unwrap().len(), 1);
    let (_, bens, _) = call(&state, "GET", "/api/v1/feedback", Some(&ben), None)
        .await
        .unwrap();
    assert_eq!(bens, json!([]), "a member sees their own reports");
    let screenshot = format!("/api/v1/feedback/{id}/screenshot");
    let (status, _, content_type) = call(&state, "GET", &screenshot, Some(&ana), None)
        .await
        .unwrap();
    assert_eq!(
        (status, content_type.as_str()),
        (StatusCode::OK, "image/png")
    );
    let (status, _, _) = call(&state, "GET", &screenshot, Some(&ops), None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = call(&state, "GET", &screenshot, Some(&ben), None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);

    customer_relations::run_once(&state).await;
    let notice = sent
        .lock()
        .unwrap()
        .iter()
        .find(|mail| mail.contains("To: ops@example.com"))
        .cloned()
        .unwrap();
    assert!(notice.contains(&format!("https://app.example/admin/feedback/{id}")));
    assert!(
        !notice.contains("wrong month"),
        "a notice never carries the report"
    );

    // With operators, the customer's controller does not read reports.
    let (status, _, _) = call(&state, "GET", "/api/v1/admin/feedback", Some(&ana), None)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, received, _) = call(
        &state,
        "GET",
        "/api/v1/admin/feedback?status=received",
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(received[0]["reporter"]["email"], "ana@kestrel.example");
    assert_eq!(received[0]["organization"]["name"], "Kestrel Digital");

    let report = format!("/api/v1/admin/feedback/{id}");
    let (_, answered, _) = call(
        &state,
        "PATCH",
        &report,
        Some(&ops),
        Some(json!({"answer": "Thanks, we reproduced it."})),
    )
    .await
    .unwrap();
    assert_eq!(answered["report"]["status"], "answered");
    customer_relations::run_once(&state).await;
    let (status, fixed, _) = call(
        &state,
        "PATCH",
        &report,
        Some(&ops),
        Some(json!({"fixed_in": "1.0.1"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixed["report"]["status"], "fixed");
    assert_eq!(fixed["report"]["answer"], "Thanks, we reproduced it.");
    customer_relations::run_once(&state).await;
    let to_ana: Vec<String> = sent
        .lock()
        .unwrap()
        .iter()
        .filter(|mail| mail.contains("To: ana@kestrel.example"))
        .cloned()
        .collect();
    assert_eq!(to_ana.len(), 2);
    assert!(
        to_ana[0].contains("has an answer") && to_ana[0].contains("https://app.example/feedback")
    );
    assert!(
        !to_ana[0].contains("reproduced"),
        "the answer is read in the app"
    );
    assert!(to_ana[1].contains("fixed in version 1.0.1"));
    let (_, mine, _) = call(&state, "GET", "/api/v1/feedback", Some(&ana), None)
        .await
        .unwrap();
    assert_eq!(mine[0]["fixed_in"], "1.0.1");

    // A full-screen screenshot fits; more than 3 MB does not.
    let mut big = PNG.to_vec();
    big.resize(2_900_000, 0);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&big);
    let (status, _, _) = call(&state, "POST", "/api/v1/feedback", Some(&ben), Some(json!({"kind": "bug", "text": "Big", "screenshot": {"type": "image/png", "data": encoded}}))).await.unwrap();
    assert_eq!(status, StatusCode::CREATED);
    big.resize(3_200_000, 0);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&big);
    let (status, _, _) = call(&state, "POST", "/api/v1/feedback", Some(&ben), Some(json!({"kind": "bug", "text": "Too big", "screenshot": {"type": "image/png", "data": encoded}}))).await.unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, other, _) = call(
        &state,
        "POST",
        "/api/v1/feedback",
        Some(&ben),
        Some(json!({"kind": "idea", "text": "Dark mode"})),
    )
    .await
    .unwrap();
    let (status, _, _) = call(
        &state,
        "PATCH",
        &format!("/api/v1/admin/feedback/{}", other["id"].as_str().unwrap()),
        Some(&ops),
        Some(json!({"status": "fixed"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST, "fixed needs the version");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn without_operators_controllers_read_their_own_reports(pool: PgPool) {
    let (smtp, sent) = fake_smtp().await.unwrap();
    let state = AppState::new(
        pool.clone(),
        config(Settings::default(), Some(smtp)),
        Arc::new(NoExtension),
    );
    let (_, _, acme) = organization(&pool, "Acme", "cfo@acme.example")
        .await
        .unwrap();
    let (kestrel, _, ana) = organization(&pool, "Kestrel Digital", "ana@kestrel.example")
        .await
        .unwrap();
    let (_, ben) = member(&pool, kestrel, "ben@kestrel.example", "agent_manager")
        .await
        .unwrap();

    let (_, created, _) = call(
        &state,
        "POST",
        "/api/v1/feedback",
        Some(&ben),
        Some(json!({"kind": "question", "text": "Where do bills go?"})),
    )
    .await
    .unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let (_, theirs, _) = call(&state, "GET", "/api/v1/admin/feedback", Some(&ana), None)
        .await
        .unwrap();
    assert_eq!(theirs.as_array().unwrap().len(), 1);
    let (_, others, _) = call(&state, "GET", "/api/v1/admin/feedback", Some(&acme), None)
        .await
        .unwrap();
    assert_eq!(others, json!([]));
    let (status, _, _) = call(
        &state,
        "GET",
        &format!("/api/v1/admin/feedback/{id}"),
        Some(&acme),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = call(&state, "GET", "/api/v1/admin/feedback", Some(&ben), None)
        .await
        .unwrap();
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "agent managers do not read reports"
    );
    let (status, _, _) = call(&state, "GET", "/api/v1/admin", Some(&ana), None)
        .await
        .unwrap();
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the rest of the admin area needs operators"
    );

    customer_relations::run_once(&state).await;
    let mails = sent.lock().unwrap().clone();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].contains("To: ana@kestrel.example"));
    assert!(
        mails[0].contains("<noreply@app.example>"),
        "the instance's own sender"
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn the_customer_record_brings_it_together(pool: PgPool) {
    let fake = fake_services(json!({})).await;
    let state = AppState::new(
        pool.clone(),
        config(settings(&fake), None),
        Arc::new(NoExtension),
    );
    let (_, _, ops) = organization(&pool, "Token Controller", "ops@example.com")
        .await
        .unwrap();
    let (kestrel, _, ana) = organization(&pool, "Kestrel Digital", "ana@kestrel.example")
        .await
        .unwrap();
    member(&pool, kestrel, "ben@kestrel.example", "agent_manager")
        .await
        .unwrap();
    sqlx::query("INSERT INTO support_conversations (channel, contact_email, subject, state) VALUES ('mail', 'Ana@Kestrel.example', 'Invoices', 'escalated'), ('mail', 'someone@else.example', 'Hi', 'new')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO newsletter_subscribers (email, source, status, confirm_token, unsubscribe_token) VALUES ('ana@kestrel.example', 'test', 'confirmed', 'c', 'u')")
        .execute(&pool)
        .await
        .unwrap();
    call(
        &state,
        "POST",
        "/api/v1/feedback",
        Some(&ana),
        Some(json!({"kind": "idea", "text": "Export to Excel"})),
    )
    .await
    .unwrap();

    let (status, note, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/customers/{kestrel}/notes"),
        Some(&ops),
        Some(json!({"text": "Call: they close monthly."})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(note["author_email"], "ops@example.com");
    let (status, _, _) = call(
        &state,
        "POST",
        &format!("/api/v1/admin/customers/{}/notes", Uuid::new_v4()),
        Some(&ops),
        Some(json!({"text": "x"})),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, customers, _) = call(&state, "GET", "/api/v1/admin/customers", Some(&ops), None)
        .await
        .unwrap();
    let listed = customers
        .as_array()
        .unwrap()
        .iter()
        .find(|customer| customer["name"] == "Kestrel Digital")
        .unwrap()
        .clone();
    assert_eq!(listed["members"], 2);
    assert_eq!(listed["open_support"], 1);
    assert_eq!(listed["open_feedback"], 1);
    assert_eq!(listed["plan"], Value::Null, "billing is off in this test");

    let (_, record, _) = call(
        &state,
        "GET",
        &format!("/api/v1/admin/customers/{kestrel}"),
        Some(&ops),
        None,
    )
    .await
    .unwrap();
    assert_eq!(record["customer"]["name"], "Kestrel Digital");
    assert_eq!(record["members"].as_array().unwrap().len(), 2);
    let ana_row = record["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["email"] == "ana@kestrel.example")
        .unwrap()
        .clone();
    assert_eq!(ana_row["newsletter"], "confirmed");
    assert_eq!(ana_row["role"], "controller");
    assert_eq!(record["support"].as_array().unwrap().len(), 1);
    assert_eq!(record["support"][0]["subject"], "Invoices");
    assert_eq!(record["feedback"][0]["report"]["text"], "Export to Excel");
    assert_eq!(record["notes"][0]["body"], "Call: they close monthly.");
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn old_screenshots_and_reports_are_deleted(pool: PgPool) {
    let state = AppState::new(
        pool.clone(),
        config(Settings::default(), None),
        Arc::new(NoExtension),
    );
    let (organization, person, _) = organization(&pool, "Kestrel Digital", "ana@kestrel.example")
        .await
        .unwrap();
    for (text, age) in [("old", "3 years"), ("month", "100 days"), ("new", "1 day")] {
        sqlx::query(
            "INSERT INTO feedback_reports (organization_id, person_id, kind, body, screenshot, screenshot_type, created_at, updated_at, reviewers_notified_at)
             VALUES ($1, $2, 'bug', $3, $4, 'image/png', now() - $5::interval, now() - $5::interval, now())",
        )
        .bind(organization)
        .bind(person)
        .bind(text)
        .bind(PNG)
        .bind(age)
        .execute(&pool)
        .await
        .unwrap();
    }

    customer_relations::run_once(&state).await;

    let left: Vec<(String, bool)> = sqlx::query_as(
        "SELECT body, screenshot IS NOT NULL FROM feedback_reports ORDER BY created_at",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        left,
        [("month".to_owned(), false), ("new".to_owned(), true)]
    );
}
