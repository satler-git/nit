use std::time::Duration;
use tokio::process::Command;

use clap::Parser;

use serde::{Deserialize, Serialize};

use ltrait::{
    Launcher, Level,
    color_eyre::{
        Result,
        eyre::{ContextCompat, ensure},
    },
};
use ltrait_extra::scorer::ScorerExt as _;
use ltrait_sorter_frecency::Frecency;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
/// The file path of nit config file is ~/.config/nix-nit/config.toml
///
/// ```toml
/// [[template]]
/// name = "test" # optional
/// uri = "github:NixOS/templates"
/// templates = ["default"] # optional. if doesn't exit, import all of templates
/// ```
struct Args {
    /// Clear and re-collect the cache(if you changed config, you have to run with re-cache)
    #[arg(short, long)]
    re_cache: bool,

    /// Display on full screen on the terminal when TUI
    #[arg(short, long, conflicts_with = "inline")]
    fullscreen: bool,

    /// How many lines to display when not in Fullscreen
    #[arg(short, long, default_value_t = 12)]
    inline: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let _guard = ltrait::setup(Level::INFO)?;
    let template = load_cache(args.re_cache).await?;

    let frecency_config = ltrait_sorter_frecency::FrecencyConfig {
        // Duration::from_secs(days * MINS_PER_HOUR * SECS_PER_MINUTE * HOURS_PER_DAY)
        half_life: Duration::from_secs(30 * 60 * 60 * 24),
        type_ident: "nix-nit".into(),
    };

    let launcher = Launcher::default()
        .batch_size(1000)
        .add_raw_source(ltrait::source::from_iter(template))
        .add_sorter(Frecency::new(frecency_config.clone())?, |c| {
            ltrait_sorter_frecency::Context {
                ident: format!("{}-{}", c.flake_info.uri, c.name),
                bonus: 15.,
            }
        })
        .add_sorter(
            ltrait_scorer_nucleo::NucleoMatcher::new(
                false,
                ltrait_scorer_nucleo::CaseMatching::Smart,
                ltrait_scorer_nucleo::Normalization::Smart,
            )
            .into_sorter(),
            |c| ltrait_scorer_nucleo::Context {
                match_string: format!(
                    "{}{}#{}",
                    if let Some(fname) = &c.flake_info.name {
                        format!("{fname} ")
                    } else {
                        String::new()
                    },
                    c.flake_info.uri,
                    c.name
                ),
            },
        )
        .add_action(Frecency::new(frecency_config)?, |c| {
            ltrait_sorter_frecency::Context {
                ident: format!("{}-{}", c.flake_info.uri, c.name),
                bonus: 15.,
            }
        })
        .add_raw_action(ltrait::action::ClosureAction::new(|t: &Template| {
            let template_uri = format!("{}#{}", t.flake_info.uri, t.name);
            let flake = std::process::Command::new("nix")
                .args(["flake", "init", "-t"])
                .arg(&template_uri)
                .output()?;

            ensure!(
                flake.status.success(),
                "failed to run nix flake init -t {template_uri}, err: {}",
                String::from_utf8(flake.stderr)?,
            );

            Ok(())
        }))
        .set_ui(
            ltrait_ui_tui::Tui::new(ltrait_ui_tui::TuiConfig::new(
                if !args.fullscreen {
                    ltrait_ui_tui::Viewport::Inline(args.inline)
                } else {
                    ltrait_ui_tui::Viewport::Fullscreen
                },
                true,
                '>',
                ' ',
                ltrait_ui_tui::sample_keyconfig,
            )),
            |c| ltrait_ui_tui::TuiEntry {
                text: (
                    format!(
                        "{}{}#{}",
                        if let Some(fname) = &c.flake_info.name {
                            format!("{fname} - ")
                        } else {
                            String::new()
                        },
                        c.flake_info.uri,
                        c.name
                    ),
                    ltrait_ui_tui::style::Style::new(),
                ),
            },
        );

    launcher.run().await?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct Config {
    template: Vec<TemplateConfig>,
}

#[derive(Debug, Deserialize)]
struct TemplateConfig {
    name: Option<String>,
    uri: String,
    templates: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    data: Vec<Template>,
}

async fn load_cache(re_cache: bool) -> Result<Vec<Template>> {
    let cache_path = dirs::cache_dir()
        .wrap_err("Cache directory does'nt exit.")?
        .join("nix-nit/cache.json");

    if re_cache || !cache_path.exists() {
        let config_path = dirs::config_dir()
            .wrap_err("Config directory  doesn't exit.")?
            .join("nix-nit/config.toml");

        ensure!(config_path.exists(), "Couldn't find a config");

        let config = toml::from_str::<Config>(&tokio::fs::read_to_string(&config_path).await?)?;
        let mut res = vec![];
        for flake in config.template {
            let mut data = load_flake(&flake.uri).await?;
            if let Some(fil) = flake.templates {
                data = data
                    .into_iter()
                    .filter(|value| fil.contains(&value.name))
                    .collect();
            }
            if let Some(name) = flake.name {
                for i in data.iter_mut() {
                    i.flake_info.name = Some(name.clone());
                }
            }
            res.extend(data);
        }

        let cache = Cache { data: res.clone() };

        {
            if let Some(parent) = cache_path.parent() {
                if !parent.as_os_str().is_empty() {
                    tokio::fs::create_dir_all(parent).await?;
                }
            }
        }

        tokio::fs::write(&cache_path, serde_json::to_string(&cache)?).await?;

        return Ok(res);
    } else {
        let data: Cache = serde_json::from_str(&tokio::fs::read_to_string(&cache_path).await?)?;
        return Ok(data.data);
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Template {
    pub name: String,
    pub flake_info: FlakeInfo,
    pub description: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct FlakeInfo {
    name: Option<String>,
    uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlakeTemplates {
    #[serde(rename = "defaultTemplate")]
    pub default_template: Template,

    pub templates: std::collections::HashMap<String, Template>,
}

async fn load_flake(flake_uri: &str) -> Result<Vec<Template>> {
    let flake = Command::new("nix")
        .args(["flake", "show"])
        .arg(flake_uri)
        .args(["--json", "--no-pretty"])
        .output()
        .await?;

    ensure!(
        flake.status.success(),
        "failed to run nix flake show {flake_uri}, err: {}",
        String::from_utf8(flake.stderr)?,
    );

    let mut flake = serde_json::from_slice::<FlakeTemplates>(&flake.stdout)?;
    let mut res = vec![];
    flake.default_template.name = "default".into();
    flake.default_template.flake_info.uri = flake_uri.to_string();
    res.push(flake.default_template);
    for (name, mut template) in flake.templates.into_iter() {
        template.name = name.clone();
        template.flake_info.uri = flake_uri.to_string();
        res.push(template);
    }

    Ok(res)
}
