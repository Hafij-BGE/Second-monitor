//! Linux backend for GNOME (Wayland or X11).
//!
//! Uses Mutter's private D-Bus APIs, the same ones gnome-remote-desktop uses:
//! - `org.gnome.Mutter.ScreenCast` `RecordVirtual` creates a real virtual
//!   monitor (extend mode); `RecordMonitor` mirrors an existing one.
//! - `org.gnome.Mutter.RemoteDesktop` injects touch input into that stream.
//!
//! Frames arrive over PipeWire as shared-memory BGRx buffers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use pipewire as pw;
use pw::spa;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::MatchRule;

use super::{Capture, CaptureRequest, Frame, FrameSlot, InputSink, Mode};
use crate::protocol::{Touch, TouchAction};

const RD_BUS: &str = "org.gnome.Mutter.RemoteDesktop";
const RD_PATH: &str = "/org/gnome/Mutter/RemoteDesktop";
const RD_SESSION_IFACE: &str = "org.gnome.Mutter.RemoteDesktop.Session";
const SC_BUS: &str = "org.gnome.Mutter.ScreenCast";
const SC_PATH: &str = "/org/gnome/Mutter/ScreenCast";
const SC_SESSION_IFACE: &str = "org.gnome.Mutter.ScreenCast.Session";
const SC_STREAM_IFACE: &str = "org.gnome.Mutter.ScreenCast.Stream";

/// Mutter cursor modes: 0 hidden, 1 embedded in frames, 2 metadata.
const CURSOR_EMBEDDED: u32 = 1;

pub fn list_monitors() -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/sys/class/drm").context("read /sys/class/drm")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        // Entries look like "card1-eDP-1"; the connector is after the first '-'.
        let Some((card, connector)) = name.split_once('-') else {
            continue;
        };
        if !card.starts_with("card") {
            continue;
        }
        let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        if status.trim() == "connected" {
            out.push(connector.to_string());
        }
    }
    out.sort();
    Ok(out)
}

pub fn start(req: &CaptureRequest) -> Result<Capture> {
    let conn = Connection::session().context("connect to the D-Bus session bus")?;

    let rd_session: OwnedObjectPath = call(&conn, RD_BUS, RD_PATH, RD_BUS, "CreateSession", &())
        .context("Mutter RemoteDesktop unavailable. Is this a GNOME session?")?;
    let session_id =
        get_string_property(&conn, RD_BUS, &rd_session, RD_SESSION_IFACE, "SessionId")?;

    let mut session_props: HashMap<&str, Value> = HashMap::new();
    session_props.insert(
        "remote-desktop-session-id",
        Value::from(session_id.as_str()),
    );
    let sc_session: OwnedObjectPath = call(
        &conn,
        SC_BUS,
        SC_PATH,
        SC_BUS,
        "CreateSession",
        &(session_props,),
    )?;

    let mut record_props: HashMap<&str, Value> = HashMap::new();
    record_props.insert("cursor-mode", Value::U32(CURSOR_EMBEDDED));
    let stream: OwnedObjectPath = match req.mode {
        Mode::Extend => {
            record_props.insert("is-platform", Value::Bool(true));
            call(
                &conn,
                SC_BUS,
                sc_session.as_str(),
                SC_SESSION_IFACE,
                "RecordVirtual",
                &(record_props,),
            )
            .context("RecordVirtual failed (needs GNOME 40 or newer)")?
        }
        Mode::Mirror => {
            let connector = match &req.monitor {
                Some(c) => c.clone(),
                None => list_monitors()?
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("no connected monitor found"))?,
            };
            log::info!("mirroring monitor {connector}");
            call(
                &conn,
                SC_BUS,
                sc_session.as_str(),
                SC_SESSION_IFACE,
                "RecordMonitor",
                &(connector.as_str(), record_props),
            )
            .with_context(|| format!("RecordMonitor({connector}) failed; see --list"))?
        }
    };

    // Subscribe before starting, or we can miss the signal.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(SC_STREAM_IFACE)?
        .member("PipeWireStreamAdded")?
        .path(stream.as_str())?
        .build();
    let signals = MessageIterator::for_match_rule(rule, &conn, Some(4))?;

    call::<()>(
        &conn,
        RD_BUS,
        rd_session.as_str(),
        RD_SESSION_IFACE,
        "Start",
        &(),
    )
    .context("failed to start the remote desktop session")?;

    let node_id = wait_for_node(signals)?;
    log::info!("PipeWire node {node_id} ready");

    let slot = Arc::new(FrameSlot::default());
    let size = Arc::new(AtomicU64::new(pack(req.width, req.height)));
    let (quit_tx, quit_rx) = pw::channel::channel::<()>();
    let want = match req.mode {
        Mode::Extend => Some((req.width, req.height)),
        Mode::Mirror => None,
    };
    {
        let slot = slot.clone();
        let size = size.clone();
        thread::Builder::new()
            .name("pipewire".into())
            .spawn(move || {
                if let Err(e) = run_pipewire(node_id, want, slot.clone(), size, quit_rx) {
                    slot.fail(format!("PipeWire capture failed: {e:#}"));
                }
            })?;
    }

    let input = MutterInput {
        conn: conn.clone(),
        session: rd_session.clone(),
        stream: stream.to_string(),
        size,
    };
    let stop = move || {
        let _ = quit_tx.send(());
        let _ = call::<()>(
            &conn,
            RD_BUS,
            rd_session.as_str(),
            RD_SESSION_IFACE,
            "Stop",
            &(),
        );
    };
    Ok(Capture {
        frames: slot,
        input: Arc::new(std::sync::Mutex::new(Box::new(input))),
        stop: Some(Box::new(stop)),
    })
}

fn wait_for_node(signals: MessageIterator) -> Result<u32> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for msg in signals {
            let result = msg
                .map_err(anyhow::Error::from)
                .and_then(|m| m.body().deserialize::<u32>().map_err(Into::into));
            let _ = tx.send(result);
            return;
        }
    });
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| anyhow!("timed out waiting for Mutter's PipeWire stream"))?
}

fn call<T>(
    conn: &Connection,
    bus: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> Result<T>
where
    T: for<'de> serde::Deserialize<'de> + zbus::zvariant::Type,
{
    let reply = conn
        .call_method(Some(bus), path, Some(iface), method, body)
        .with_context(|| format!("{iface}.{method}"))?;
    Ok(reply.body().deserialize::<T>()?)
}

fn get_string_property(
    conn: &Connection,
    bus: &str,
    path: &str,
    iface: &str,
    name: &str,
) -> Result<String> {
    let value: OwnedValue = call(
        conn,
        bus,
        path,
        "org.freedesktop.DBus.Properties",
        "Get",
        &(iface, name),
    )?;
    Ok(String::try_from(value)?)
}

fn pack(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | h as u64
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

struct MutterInput {
    conn: Connection,
    session: OwnedObjectPath,
    stream: String,
    /// Current stream size in pixels, updated on format changes.
    size: Arc<AtomicU64>,
}

impl InputSink for MutterInput {
    fn touch(&mut self, t: Touch) {
        let (w, h) = unpack(self.size.load(Ordering::Relaxed));
        let x = t.x as f64 * w.saturating_sub(1) as f64;
        let y = t.y as f64 * h.saturating_sub(1) as f64;
        let slot = t.id as u32;
        let s = self.stream.as_str();
        let path = self.session.as_str();
        let result = match t.action {
            TouchAction::Down => call::<()>(
                &self.conn,
                RD_BUS,
                path,
                RD_SESSION_IFACE,
                "NotifyTouchDown",
                &(s, slot, x, y),
            ),
            TouchAction::Move => call::<()>(
                &self.conn,
                RD_BUS,
                path,
                RD_SESSION_IFACE,
                "NotifyTouchMotion",
                &(s, slot, x, y),
            ),
            TouchAction::Up | TouchAction::Cancel => call::<()>(
                &self.conn,
                RD_BUS,
                path,
                RD_SESSION_IFACE,
                "NotifyTouchUp",
                &(slot,),
            ),
        };
        if let Err(e) = result {
            log::debug!("touch injection failed: {e:#}");
        }
    }
}

struct StreamState {
    format: spa::param::video::VideoInfoRaw,
    slot: Arc<FrameSlot>,
    size: Arc<AtomicU64>,
}

fn run_pipewire(
    node_id: u32,
    want: Option<(u32, u32)>,
    slot: Arc<FrameSlot>,
    size: Arc<AtomicU64>,
    quit_rx: pw::channel::Receiver<()>,
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let _quit = quit_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });

    let stream = pw::stream::StreamBox::new(
        &core,
        "second-monitor",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;

    let state = StreamState {
        format: Default::default(),
        slot,
        size,
    };
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(|_, state, _, new| {
            log::debug!("PipeWire stream state: {new:?}");
            if let pw::stream::StreamState::Error(e) = new {
                state.slot.fail(format!("PipeWire stream error: {e}"));
            }
        })
        .param_changed(|_, state, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            if state.format.parse(param).is_ok() {
                let s = state.format.size();
                log::info!(
                    "stream format {:?} {}x{}",
                    state.format.format(),
                    s.width,
                    s.height
                );
                state.size.store(pack(s.width, s.height), Ordering::Relaxed);
            }
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let chunk = data.chunk();
            if chunk.size() == 0 {
                return; // cursor-only or empty update
            }
            let offset = chunk.offset() as usize;
            let s = state.format.size();
            let (w, h) = (s.width as usize, s.height as usize);
            let stride = match chunk.stride() {
                st if st > 0 => st as usize,
                _ => w * 4,
            };
            let swap_rb = matches!(
                state.format.format(),
                spa::param::video::VideoFormat::RGBx | spa::param::video::VideoFormat::RGBA
            );
            let Some(bytes) = data.data() else { return };
            if w == 0 || h == 0 || offset + stride * (h - 1) + w * 4 > bytes.len() {
                return;
            }
            let mut bgra = Vec::with_capacity(w * h * 4);
            for row in 0..h {
                let start = offset + row * stride;
                bgra.extend_from_slice(&bytes[start..start + w * 4]);
            }
            if swap_rb {
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
            }
            state.slot.put(Frame {
                width: w as u32,
                height: h as u32,
                bgra,
            });
        })
        .register()?;

    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    use spa::param::video::VideoFormat;
    use spa::pod::{property, Pod, Value as PodValue};
    use spa::utils::{Fraction, Rectangle, SpaTypes};

    let format = spa::pod::property!(
        FormatProperties::VideoFormat,
        Choice,
        Enum,
        Id,
        VideoFormat::BGRx,
        VideoFormat::BGRx,
        VideoFormat::BGRA,
        VideoFormat::RGBx,
        VideoFormat::RGBA
    );
    let framerate = property!(
        FormatProperties::VideoFramerate,
        Choice,
        Range,
        Fraction,
        Fraction { num: 60, denom: 1 },
        Fraction { num: 0, denom: 1 },
        Fraction {
            num: 1000,
            denom: 1
        }
    );
    // A fixed size tells Mutter how big to make the virtual monitor.
    let video_size = match want {
        Some((width, height)) => {
            property!(
                FormatProperties::VideoSize,
                Rectangle,
                Rectangle { width, height }
            )
        }
        None => property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle {
                width: 1920,
                height: 1080
            },
            Rectangle {
                width: 1,
                height: 1
            },
            Rectangle {
                width: 8192,
                height: 8192
            }
        ),
    };
    let obj = spa::pod::object!(
        SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        property!(FormatProperties::MediaType, Id, MediaType::Video),
        property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        format,
        video_size,
        framerate,
    );
    let bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &PodValue::Object(obj),
    )
    .map_err(|e| anyhow!("serialize format pod: {e:?}"))?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("invalid format pod"))?];

    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    mainloop.run();
    if let Err(e) = stream.disconnect() {
        bail!("disconnect: {e}");
    }
    Ok(())
}
