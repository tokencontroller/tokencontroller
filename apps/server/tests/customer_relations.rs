//! The support inbox, the site assistant and the newsletter against a real
//! Postgres database, with the Claude API, Resend and the docs site faked by
//! a local HTTP server and the mail server by a local SMTP server.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hmac::{KeyInit as _, Mac as _};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::{PgPool, Row as _};
use tokencontroller_core::NoExtension;
use tokencontroller_server::customer_relations::{self, Settings};
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
