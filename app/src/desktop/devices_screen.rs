//! The Devices screen: a phone or a tablet paired with the app (devices). How Tailscale stands on
//! this computer and a button that sets `tailscale serve` up, a QR code that pairs a device once,
//! and the paired devices with Remove.

use device_protocol::{PAIRING_CODE_SECONDS, PORT, PairingLink};
use iced::widget::{Column, button, column, container, qr_code, row, text};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, CARD_PADDING, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::devices::device_model::Device;
use crate::devices::tailscale::TailscaleState;
use crate::devices::{device_service, tailscale};
use crate::matches::dates;

/// Seconds in a minute, which the pairing code's time is told in.
const MINUTE_SECONDS: i64 = 60;
/// The size of one of the QR code's cells, in pixels.
const QR_CELL_SIZE: f32 = 6.0;

#[derive(Default)]
pub struct State {
    /// None until read.
    tailscale: Option<TailscaleState>,
    /// None until read; Err says why they couldn't be.
    devices: Option<Result<Vec<Device>, String>>,
    /// The pairing code shown now: its QR code, and its link.
    pairing: Option<(qr_code::Data, String)>,
    /// Setting up `tailscale serve`, pairing or removing: their buttons wait.
    busy: bool,
    said: Option<(Health, String)>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Load,
    Loaded(Box<(TailscaleState, Result<Vec<Device>, String>)>),
    Serve,
    Served(Result<(), String>),
    Pair,
    /// The pairing link to show, or why there is none.
    Paired(Result<String, String>),
    Remove(i64),
    Removed(Result<(), String>),
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            return runner.run(
                |context| async move {
                    let tailscale = tailscale::state(&context, PORT, now()).await;
                    let devices = device_service::devices(&context).await.map_err(|e| e.message);
                    (tailscale, devices)
                },
                |read| match read {
                    Ok(read) => Msg::Loaded(Box::new(read)),
                    Err(why) => Msg::Loaded(Box::new((TailscaleState::Missing(why.clone()), Err(why)))),
                },
            );
        }
        Msg::Loaded(read) => {
            let (tailscale, devices) = *read;
            state.tailscale = Some(tailscale);
            state.devices = Some(devices);
        }
        Msg::Serve => {
            state.busy = true;
            return runner.run(
                |_| async move { tailscale::serve(PORT).await },
                |served| Msg::Served(served.and_then(|served| served)),
            );
        }
        Msg::Served(served) => {
            state.busy = false;
            state.said = Some(match served {
                Ok(()) => (
                    Health::Ready,
                    "Tailscale passes the device address on to JobFinder now, and keeps doing so after a restart."
                        .to_string(),
                ),
                Err(why) => {
                    crate::log_line!("devices: tailscale serve: {why}");
                    (
                        Health::Attention,
                        format!(
                            "{why} Run it yourself in a terminal: {}",
                            tailscale::serve_command(PORT)
                        ),
                    )
                }
            });
            return Task::done(Msg::Load);
        }
        Msg::Pair => {
            return runner.run(
                |context| async move {
                    device_service::new_pairing_link(&context, now())
                        .map(|link| link.to_link())
                        .map_err(|e| e.message)
                },
                |link| Msg::Paired(link.and_then(|link| link)),
            );
        }
        Msg::Paired(Ok(link)) => match qr_code::Data::new(&link) {
            Ok(data) => {
                state.said = None;
                state.pairing = Some((data, link));
            }
            Err(e) => {
                crate::log_line!("devices: the pairing code couldn't be drawn: {e}");
                state.said = Some((Health::Attention, format!("The pairing code couldn't be drawn: {e}")));
            }
        },
        Msg::Paired(Err(why)) => {
            state.pairing = None;
            state.said = Some((Health::Attention, why));
        }
        Msg::Remove(id) => {
            state.busy = true;
            return runner.run(
                move |context| async move { device_service::remove(&context, id).await.map_err(|e| e.message) },
                |removed| Msg::Removed(removed.and_then(|removed| removed)),
            );
        }
        Msg::Removed(removed) => {
            state.busy = false;
            state.said = Some(match removed {
                Ok(()) => (
                    Health::Ready,
                    "The device is removed; its app can't reach JobFinder any more.".to_string(),
                ),
                Err(why) => (Health::Attention, why),
            });
            return Task::done(Msg::Load);
        }
    }
    Task::none()
}

/// Clears what the last visit showed: an old pairing code never shows again.
pub fn shown(state: &mut State) {
    state.pairing = None;
    state.said = None;
}

pub fn view(state: &State, not_listening: Option<String>) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Devices",
        widgets::note(
            "Your phone or tablet, with the JobFinder app: it shows your shortlist and sends job posts here to evaluate. It reaches this computer through Tailscale, wherever you are.",
        ),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    if let Some(why) = not_listening {
        screen = screen.push(widgets::message_bar(Health::Attention, why));
    }
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    screen
        .push(widgets::section("Tailscale", padded(tailscale_part(state))))
        .push(widgets::section("Pair a device", padded(pairing_part(state))))
        .push(widgets::section("Paired devices", paired_part(state)))
        .into()
}

/// How Tailscale stands, and what to do about it.
fn tailscale_part(state: &State) -> Element<'_, Msg> {
    let Some(tailscale) = &state.tailscale else {
        return widgets::note("Looking at Tailscale…");
    };
    match tailscale {
        TailscaleState::Missing(why) => column![
            line(Health::Attention, "Tailscale isn't installed on this computer, or couldn't be asked."),
            widgets::caption(why.as_str()),
            widgets::caption("Install it from tailscale.com, sign in, and install the Tailscale app on your phone with the same account."),
        ]
        .spacing(6)
        .into(),
        TailscaleState::NotRunning(backend) => column![
            line(Health::Attention, "Tailscale is installed but not connected."),
            widgets::caption(format!("It says {backend}. Open Tailscale and sign in.")),
        ]
        .spacing(6)
        .into(),
        TailscaleState::Running { name, serving: true } => column![
            line(Health::Ready, "Tailscale passes the device address on to JobFinder."),
            widgets::caption(format!("Devices reach this computer at https://{name}:{PORT}.")),
        ]
        .spacing(6)
        .into(),
        TailscaleState::Running { name, serving: false } => {
            let mut serve = button(text("Set up tailscale serve").size(style::BODY_SIZE)).style(style::primary_button);
            if !state.busy {
                serve = serve.on_press(Msg::Serve);
            }
            column![
                line(Health::Attention, "Tailscale runs, but doesn't pass the device address on to JobFinder yet."),
                widgets::caption(format!(
                    "The button runs \"{}\", so https://{name}:{PORT} reaches JobFinder from your devices, and nothing else. It needs MagicDNS and HTTPS certificates turned on in the Tailscale admin console.",
                    tailscale::serve_command(PORT)
                )),
                serve,
            ]
            .spacing(10)
            .into()
        }
    }
}

/// The QR code shown now, or the button that shows one.
fn pairing_part(state: &State) -> Element<'_, Msg> {
    let minutes = PAIRING_CODE_SECONDS / MINUTE_SECONDS;
    let label = if state.pairing.is_some() {
        "Show a new pairing code"
    } else {
        "Show a pairing code"
    };
    let mut show = button(text(label).size(style::BODY_SIZE)).style(style::standard_button);
    if !state.busy {
        show = show.on_press(Msg::Pair);
    }
    match &state.pairing {
        None => column![
            widgets::caption(format!(
                "Open the JobFinder app on the device and scan the code this shows. It works once, for {minutes} minutes."
            )),
            show,
        ]
        .spacing(10)
        .into(),
        Some((data, link)) => {
            let host = PairingLink::parse(link).map(|link| link.address()).unwrap_or_default();
            column![
                container(qr_code(data).cell_size(QR_CELL_SIZE)).center_x(Fill),
                widgets::caption(format!(
                    "Scan this with the JobFinder app on the device. It works once, for {minutes} minutes, and reaches {host}."
                )),
                show,
            ]
            .spacing(10)
            .align_x(Center)
            .into()
        }
    }
}

/// The paired devices, each with Remove.
fn paired_part(state: &State) -> Element<'_, Msg> {
    let devices = match &state.devices {
        None => return padded(widgets::note("Reading…")),
        Some(Err(why)) => return padded(widgets::errors_line(why.as_str())),
        Some(Ok(devices)) if devices.is_empty() => return padded(widgets::note("No device is paired yet.")),
        Some(Ok(devices)) => devices,
    };
    widgets::rows(devices.iter().map(|device| device_row(device, state.busy)))
}

fn device_row(device: &Device, busy: bool) -> Element<'_, Msg> {
    let seen = device.last_seen_at.map_or_else(
        || "not seen since".to_string(),
        |at| format!("last seen {}", dates::moment(at)),
    );
    let mut remove = button(text("Remove").size(style::BODY_SIZE)).style(style::danger_button);
    if !busy {
        remove = remove.on_press(Msg::Remove(device.id));
    }
    row![
        column![
            text(device.name.as_str())
                .font(style::semibold())
                .size(style::BODY_SIZE),
            widgets::caption(format!("Paired {}, {seen}", dates::day(device.paired_at))),
        ]
        .spacing(2)
        .width(Fill),
        remove,
    ]
    .align_y(Center)
    .padding(ROW_PADDING)
    .into()
}

/// A line with its mark.
fn line<'a>(health: Health, said: &'a str) -> Element<'a, Msg> {
    row![style::mark(health), text(said).size(style::BODY_SIZE).width(Fill)]
        .spacing(8)
        .align_y(Center)
        .into()
}

fn padded(body: Element<'_, Msg>) -> Element<'_, Msg> {
    container(body).padding(CARD_PADDING).width(Fill).into()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
