# A model call that carries prompt text keeps its counts and loses the text

Prompt text still never enters. What changes is what the ingest endpoint does with a telemetry event or span that carries some:

- **A model call** (the one event or span per call that each agent harness counts from) loses every attribute that carries prompt text, at any depth, and its body. The token counts, model, session and the other attributes are read from what is left, and the call is counted. The answer to the export names how many calls lost text and asks to turn prompt logging off, without rejecting them.
- **Any other event or span** that carries prompt text is still dropped whole and counted as rejected.

`otlp::privacy::PROMPT_TEXT_ATTRIBUTES` decides what is text, as before, and nothing that carries it reaches the database.

## Why

Gemini CLI logs prompts unless `logPrompts` is set to false, and then puts the response text on the same `gemini_cli.api_response` event that holds the token counts. Dropping that event whole meant a team running Gemini CLI with its default settings saw no cost at all, with no sign in the web app. Copilot CLI does the same on its `chat` spans when message capture is on. Removing the text keeps the privacy promise, since the text is never stored either way, and keeps the money.
