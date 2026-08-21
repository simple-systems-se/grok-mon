mod app;
mod auth;
mod billing;
mod config;
mod sessions;

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(
                        "warn,cosmic_ext_applet_grok_monitor=info",
                    )
                }),
        )
        .init();
    tracing::info!("starting grok monitor applet");
    cosmic::applet::run::<app::GrokMonitor>(())
}
