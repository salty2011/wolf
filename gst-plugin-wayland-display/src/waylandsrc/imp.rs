use std::sync::{
    mpsc::{self, SyncSender},
    Mutex,
};
use std::thread::JoinHandle;

use gst_video::{VideoCapsBuilder, VideoFormat, VideoInfo};
use smithay::backend::{drm::DrmNode, SwapBuffersError};
use smithay::reexports::calloop::channel::Sender;

use gst::glib::{once_cell::sync::Lazy, ValueArray};
use gst::prelude::*;
use gst::subclass::prelude::*;
use gst::{glib, Event, Fraction};

use gst_base::subclass::base_src::CreateSuccess;
use gst_base::subclass::prelude::*;
use gst_base::traits::BaseSrcExt;

use crate::utils::CAT;

use super::comp::RenderTarget;

pub struct WaylandDisplaySrc {
    state: Mutex<Option<State>>,
    settings: Mutex<Settings>,
}

impl Default for WaylandDisplaySrc {
    fn default() -> Self {
        WaylandDisplaySrc {
            state: Mutex::new(None),
            settings: Mutex::new(Settings::default()),
        }
    }
}

#[derive(Debug, Default)]
pub struct Settings {
    render_node: Option<RenderTarget>,
}

pub struct State {
    thread_handle: JoinHandle<()>,
    command_tx: Sender<Command>,
}

pub enum Command {
    VideoInfo(VideoInfo),
    InputDevice(String),
    Buffer(SyncSender<Result<gst::Buffer, SwapBuffersError>>),
    Quit,
}

#[glib::object_subclass]
impl ObjectSubclass for WaylandDisplaySrc {
    const NAME: &'static str = "GstWaylandDisplaySrc";
    type Type = super::WaylandDisplaySrc;
    type ParentType = gst_base::PushSrc;
    type Interfaces = ();
}

impl ObjectImpl for WaylandDisplaySrc {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![glib::ParamSpecString::builder("render-node")
                .nick("DRM Render Node")
                .blurb("DRM Render Node to use (e.g. /dev/dri/renderD128")
                .construct()
                .build()]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        match pspec.name() {
            "render-node" => {
                let mut settings = self.settings.lock().unwrap();
                let node = value
                    .get::<Option<String>>()
                    .expect("type checked upstream")
                    .map(|path| match &*path {
                        "software" => RenderTarget::Software,
                        path => RenderTarget::Hardware(
                            DrmNode::from_path(path).expect("Not a valid render_node"),
                        ),
                    });
                settings.render_node = node;
            }
            _ => unreachable!(),
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "render-node" => {
                let settings = self.settings.lock().unwrap();
                settings
                    .render_node
                    .as_ref()
                    .and_then(|target| match target {
                        RenderTarget::Software => Some(String::from("software")),
                        RenderTarget::Hardware(node) => node
                            .dev_path()
                            .map(|path| path.to_string_lossy().into_owned()),
                    })
                    .unwrap_or_else(|| String::from("/dev/dri/renderD128"))
                    .to_value()
            }
            _ => unreachable!(),
        }
    }

    fn constructed(&self) {
        self.parent_constructed();

        let obj = self.obj();
        obj.set_element_flags(gst::ElementFlags::SOURCE);
        obj.set_live(true);
        obj.set_format(gst::Format::Time);
        obj.set_automatic_eos(false);
        obj.set_do_timestamp(true);
    }
}

impl GstObjectImpl for WaylandDisplaySrc {}

impl ElementImpl for WaylandDisplaySrc {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: Lazy<gst::subclass::ElementMetadata> = Lazy::new(|| {
            gst::subclass::ElementMetadata::new(
                "Wayland display source",
                "Source/Video",
                "GStreamer video src running a wayland compositor",
                "Victoria Brekenfeld <wayland@drakulix.de>",
            )
        });

        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: Lazy<Vec<gst::PadTemplate>> = Lazy::new(|| {
            let caps = gst_video::VideoCapsBuilder::new()
                .format(VideoFormat::Rgbx)
                .height_range(..i32::MAX)
                .width_range(..i32::MAX)
                .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
                .build();
            let src_pad_template = gst::PadTemplate::new(
                "src",
                gst::PadDirection::Src,
                gst::PadPresence::Always,
                &caps,
            )
            .unwrap();

            vec![src_pad_template]
        });

        PAD_TEMPLATES.as_ref()
    }

    fn change_state(
        &self,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        let res = self.parent_change_state(transition);
        match res {
            Ok(gst::StateChangeSuccess::Success) => {
                if transition.next() == gst::State::Paused {
                    // this is a live source
                    Ok(gst::StateChangeSuccess::NoPreroll)
                } else {
                    Ok(gst::StateChangeSuccess::Success)
                }
            }
            x => x,
        }
    }

    fn query(&self, query: &mut gst::QueryRef) -> bool {
        ElementImplExt::parent_query(self, query)
    }
}

impl BaseSrcImpl for WaylandDisplaySrc {
    fn query(&self, query: &mut gst::QueryRef) -> bool {
        BaseSrcImplExt::parent_query(self, query)
    }

    fn caps(&self, filter: Option<&gst::Caps>) -> Option<gst::Caps> {
        let mut caps = VideoCapsBuilder::new()
            .format(VideoFormat::Rgbx)
            .height_range(..i32::MAX)
            .width_range(..i32::MAX)
            .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
            .build();

        if let Some(filter) = filter {
            caps = caps.intersect(filter);
        }

        Some(caps)
    }

    fn negotiate(&self) -> Result<(), gst::LoggableError> {
        self.parent_negotiate()
    }

    fn event(&self, event: &Event) -> bool {
        if event.type_() == gst::EventType::CustomUpstream {
            let structure = event.structure().expect("Unable to get message structure");
            if structure.has_name("VirtualDevicesReady") {
                let mut state = self.state.lock().unwrap();
                let tx = &mut state.as_mut().unwrap().command_tx;

                let paths = structure
                    .get::<ValueArray>("paths")
                    .expect("Should contain paths");
                for value in paths.into_iter() {
                    let path = value.get::<String>().expect("Paths are strings");
                    if let Err(err) = tx.send(Command::InputDevice(path)) {
                        gst::warning!(CAT, "Command channel dead: {}", err);
                    }
                }

                return true;
            }
        }
        self.parent_event(event)
    }

    fn set_caps(&self, caps: &gst::Caps) -> Result<(), gst::LoggableError> {
        let video_info = gst_video::VideoInfo::from_caps(caps).expect("failed to get video info");
        let _ = self
            .state
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .command_tx
            .send(Command::VideoInfo(video_info));

        self.parent_set_caps(caps)
    }

    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();
        if state.is_some() {
            return Ok(());
        }

        let settings = self.settings.lock().unwrap();
        let render_target = settings.render_node.clone().unwrap_or_else(||
            DrmNode::from_path("/dev/dri/renderD128").expect("Unable to open dri node. Set `render-node=software` to use software rendering.").into(),
        );

        let elem = self.obj().upcast_ref::<gst::Element>().to_owned();
        let (tx, rx) = std::sync::mpsc::sync_channel(0);
        let thread_handle = std::thread::spawn(move || {
            if let Err(err) = std::panic::catch_unwind(|| {
                // calloops channel is not "UnwindSafe", but the std channel is... *sigh* lets workaround it creatively
                let (command_tx, command_src) = smithay::reexports::calloop::channel::channel();
                tx.send(command_tx).unwrap();
                super::comp::init(command_src, render_target, elem);
            }) {
                gst::error!(CAT, "Compositor thread panic'ed: {:?}", err);
            }
        });
        let command_tx = rx.recv().unwrap();

        *state = Some(State {
            thread_handle,
            command_tx,
        });

        Ok(())
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();
        if let Some(state) = state.take() {
            if let Err(err) = state.command_tx.send(Command::Quit) {
                gst::warning!(CAT, "Failed to send stop command: {}", err);
                return Ok(());
            };
            if state.thread_handle.join().is_err() {
                gst::warning!(CAT, "Failed to join compositor thread");
            };
            std::mem::drop(state.command_tx);
        }

        Ok(())
    }

    fn is_seekable(&self) -> bool {
        false
    }
}

impl PushSrcImpl for WaylandDisplaySrc {
    fn create(
        &self,
        _buffer: Option<&mut gst::BufferRef>,
    ) -> Result<CreateSuccess, gst::FlowError> {
        let mut state_guard = self.state.lock().unwrap();
        let Some(state) = state_guard.as_mut() else {
            return Err(gst::FlowError::Eos);
        };

        let (buffer_tx, buffer_rx) = mpsc::sync_channel(0);
        if let Err(err) = state.command_tx.send(Command::Buffer(buffer_tx)) {
            gst::warning!(CAT, "Failed to send buffer command: {}", err);
            return Err(gst::FlowError::Eos);
        }

        match buffer_rx.recv() {
            Ok(Ok(buffer)) => Ok(CreateSuccess::NewBuffer(buffer)),
            Ok(Err(err)) => match err {
                SwapBuffersError::AlreadySwapped => unreachable!(),
                SwapBuffersError::ContextLost(_) => Err(gst::FlowError::Eos),
                SwapBuffersError::TemporaryFailure(_) => Err(gst::FlowError::Error),
            },
            Err(err) => {
                gst::warning!(CAT, "Failed to recv buffer ack: {}", err);
                Err(gst::FlowError::Error)
            }
        }
    }
}
