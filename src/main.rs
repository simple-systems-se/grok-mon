mod app;
mod auth;
mod billing;
mod bot;
mod config;
mod ring;
mod sessions;
mod spawn;

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("warn,cosmic_ext_applet_grok_monitor=info")
            }),
        )
        .init();
    if is_bot_product() {
        tracing::info!("starting grok bot monitor applet");
        cosmic::applet::run::<bot::app::GrokBotMonitor>(())
    } else {
        tracing::info!("starting grok monitor applet");
        cosmic::applet::run::<app::GrokMonitor>(())
    }
}

fn is_bot_product() -> bool {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--product=bot" {
            return true;
        }
        if arg == "--product" {
            return args.next().as_deref() == Some("bot");
        }
    }
    false
}
