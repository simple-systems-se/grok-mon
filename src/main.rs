mod api;
mod app;
mod auth;
mod billing;
mod bot;
mod config;
mod ring;
mod sessions;
mod spawn;

enum Product {
    Build,
    Bot,
    Api,
}

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("warn,cosmic_ext_applet_grok_monitor=info")
            }),
        )
        .init();
    match product() {
        Product::Bot => {
            tracing::info!("starting grok bot monitor applet");
            cosmic::applet::run::<bot::app::GrokBotMonitor>(())
        }
        Product::Api => {
            tracing::info!("starting grok api monitor applet");
            cosmic::applet::run::<api::app::GrokApiMonitor>(())
        }
        Product::Build => {
            tracing::info!("starting grok monitor applet");
            cosmic::applet::run::<app::GrokMonitor>(())
        }
    }
}

fn product() -> Product {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = if let Some(rest) = arg.strip_prefix("--product=") {
            Some(rest.to_string())
        } else if arg == "--product" {
            args.next()
        } else {
            None
        };
        if let Some(value) = value {
            return match value.as_str() {
                "bot" => Product::Bot,
                "api" => Product::Api,
                _ => Product::Build,
            };
        }
    }
    Product::Build
}
