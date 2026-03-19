use std::os::fd::OwnedFd;
use std::sync::mpsc;

use eyre::{Context, Result};
use pipewire as pw;
use pw::spa;
use pw::spa::pod::Pod;

use crate::config::Config;

/// Result of portal negotiation: the PipeWire fd and node ID to connect to.
pub struct CaptureSession {
    pub fd: OwnedFd,
    pub node_id: u32,
    pub restore_token: Option<String>,
}

/// Frame data received from PipeWire.
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

/// Negotiate a screen capture session via XDG Desktop Portal.
pub async fn create_session(config: &Config) -> Result<CaptureSession> {
    use ashpd::desktop::PersistMode;
    use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};

    let proxy = Screencast::new()
        .await
        .context("Failed to connect to screencast portal")?;

    let session = proxy
        .create_session(Default::default())
        .await
        .context("Failed to create screencast session")?;

    let mut select_opts = SelectSourcesOptions::default()
        .set_cursor_mode(CursorMode::Embedded)
        .set_sources(enumflags2::BitFlags::from(SourceType::Monitor))
        .set_multiple(false)
        .set_persist_mode(PersistMode::ExplicitlyRevoked);

    if let Some(token) = &config.portal_restore_token {
        select_opts = select_opts.set_restore_token(token.as_str());
    }

    proxy
        .select_sources(&session, select_opts)
        .await
        .context("Failed to select screencast sources")?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .context("Screencast start failed")?
        .response()
        .context("Screencast start response failed")?;

    let streams = response.streams();
    if streams.is_empty() {
        eyre::bail!("No screencast streams returned by portal");
    }

    let stream = &streams[0];
    let node_id = stream.pipe_wire_node_id();
    let restore_token = response.restore_token().map(String::from);

    log::info!("Portal session started: node_id={}, size={:?}", node_id, stream.size());

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .context("Failed to open PipeWire remote")?;

    Ok(CaptureSession {
        fd,
        node_id,
        restore_token,
    })
}

struct StreamUserData {
    format: spa::param::video::VideoInfoRaw,
    width: u32,
    height: u32,
    tx: mpsc::SyncSender<Frame>,
}

/// Run the PipeWire capture loop, sending frames to the provided channel.
pub fn run_pipewire_stream(session: CaptureSession, frame_tx: mpsc::SyncSender<Frame>) -> Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).context("Failed to create PipeWire main loop")?;
    let context = pw::context::ContextRc::new(&mainloop, None).context("Failed to create PipeWire context")?;
    let core = context
        .connect_fd_rc(session.fd, None)
        .context("Failed to connect to PipeWire via fd")?;

    let stream = pw::stream::StreamBox::new(
        &core,
        "viewport2-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .context("Failed to create PipeWire stream")?;

    let user_data = StreamUserData {
        format: Default::default(),
        width: 0,
        height: 0,
        tx: frame_tx,
    };

    let listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(|_, _, old, new| {
            log::info!("PipeWire stream state: {:?} -> {:?}", old, new);
        })
        .param_changed(|_, state, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param) else {
                return;
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            if let Err(e) = state.format.parse(param) {
                log::error!("Failed to parse video format: {}", e);
                return;
            }
            let size = state.format.size();
            state.width = size.width;
            state.height = size.height;
            log::info!(
                "Negotiated format: {:?} {}x{} @ {}/{}fps",
                state.format.format(),
                size.width,
                size.height,
                state.format.framerate().num,
                state.format.framerate().denom,
            );
        })
        .process(|stream, state| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];
                let chunk = data.chunk();
                let size = chunk.size() as usize;
                let stride = chunk.stride() as u32;
                let offset = chunk.offset() as usize;

                if let Some(slice) = data.data()
                    && offset + size <= slice.len()
                    && state.width > 0
                    && state.height > 0
                {
                    let pixels = &slice[offset..offset + size];
                    let frame = Frame {
                        data: pixels.to_vec(),
                        width: state.width,
                        height: state.height,
                        stride,
                    };
                    // Non-blocking send - drop frames if consumer is slow
                    let _ = state.tx.try_send(frame);
                }
            }
        })
        .register()
        .context("Failed to register PipeWire stream listener")?;

    // Build format pod
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGB,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle { width: 1, height: 1 },
            spa::utils::Rectangle {
                width: 7680,
                height: 4320
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 60, denom: 1 }
        ),
    );

    let values: Vec<u8> =
        spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(obj))
            .context("Failed to serialize format pod")?
            .0
            .into_inner();

    let mut params = [Pod::from_bytes(&values).expect("Failed to parse format pod")];

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(session.node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .context("Failed to connect PipeWire stream")?;

    log::info!("PipeWire stream connected, entering main loop");
    mainloop.run();

    // Listener must stay alive for the duration of the mainloop
    drop(listener);

    Ok(())
}
