//! `tc config [KEY [VALUE]]`: show and change settings.

use crate::app::App;
use crate::error::Result;
use crate::home::SETTINGS;
use crate::output;

pub fn run(app: &mut App<'_>, key: Option<&str>, value: Option<&str>) -> Result<()> {
    match (key, value) {
        (None, _) => {
            let all = app.config.all();
            if app.json {
                return output::json(app.out, &all);
            }
            for (name, about) in SETTINGS {
                let value = all.get(name).cloned().flatten();
                writeln!(
                    app.out,
                    "{name} = {}",
                    value.as_deref().unwrap_or("(not set)")
                )?;
                writeln!(app.out, "    {about}")?;
            }
            Ok(())
        }
        (Some(key), None) => {
            let value = app.config.get(key)?;
            if app.json {
                return output::json(app.out, &serde_json::json!({ "key": key, "value": value }));
            }
            writeln!(app.out, "{}", value.as_deref().unwrap_or("(not set)"))?;
            Ok(())
        }
        (Some(key), Some(value)) => {
            app.config.set(key, value)?;
            app.home.write_config(&app.config)?;
            let value = app.config.get(key)?;
            if app.json {
                return output::json(app.out, &serde_json::json!({ "key": key, "value": value }));
            }
            match value {
                Some(value) => writeln!(app.out, "{key} = {value}")?,
                None => writeln!(app.out, "{key} removed")?,
            }
            Ok(())
        }
    }
}
