use pyroscope::{
    backend::{Backend, Report, StackFrame, StackTrace, State},
    error::{PyroscopeError, Result},
};
use rbspy::sampler::Sampler;
use std::sync::{
    mpsc::{channel, sync_channel, Receiver, Sender, SyncSender},
    Arc, Mutex,
};

/// Rbspy Configuration
#[derive(Debug)]
pub struct RbspyConfig {
    /// Process to monitor
    pid: Option<i32>,
    /// Sampling rate
    sample_rate: u32,
    /// Lock Process while sampling
    lock_process: bool,
    /// Profiling duration. None for infinite.
    time_limit: Option<core::time::Duration>,
    /// Include subprocesses
    with_subprocesses: bool,
}

impl Default for RbspyConfig {
    fn default() -> Self {
        RbspyConfig {
            pid: None,
            sample_rate: 100,
            lock_process: false,
            time_limit: None,
            with_subprocesses: false,
        }
    }
}

impl RbspyConfig {
    /// Create a new RbspyConfig
    pub fn new(pid: i32) -> Self {
        RbspyConfig {
            pid: Some(pid),
            ..Default::default()
        }
    }

    pub fn sample_rate(self, sample_rate: u32) -> Self {
        RbspyConfig {
            sample_rate,
            ..self
        }
    }

    pub fn lock_process(self, lock_process: bool) -> Self {
        RbspyConfig {
            lock_process,
            ..self
        }
    }

    pub fn time_limit(self, time_limit: Option<core::time::Duration>) -> Self {
        RbspyConfig { time_limit, ..self }
    }

    pub fn with_subprocesses(self, with_subprocesses: bool) -> Self {
        RbspyConfig {
            with_subprocesses,
            ..self
        }
    }
}

/// Rbspy Backend
#[derive(Default)]
pub struct Rbspy {
    /// Rbspy State
    state: State,
    /// Rbspy Configuration
    config: RbspyConfig,
    /// Rbspy Sampler
    sampler: Option<Sampler>,
    /// StackTrace Receiver
    stack_receiver: Option<Receiver<rbspy::StackTrace>>,
    /// Error Receiver
    error_receiver: Option<Receiver<std::result::Result<(), anyhow::Error>>>,
    /// Profiling buffer
    buffer: Arc<Mutex<Report>>,
}

impl std::fmt::Debug for Rbspy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rbspy Backend")
    }
}

impl Rbspy {
    pub fn new(config: RbspyConfig) -> Self {
        Rbspy {
            sampler: None,
            stack_receiver: None,
            error_receiver: None,
            state: State::Uninitialized,
            config,
            buffer: Arc::new(Mutex::new(Report::default())),
        }
    }
}

// Type aliases
type ErrorSender = Sender<std::result::Result<(), anyhow::Error>>;
type ErrorReceiver = Receiver<std::result::Result<(), anyhow::Error>>;

impl Backend for Rbspy {
    fn get_state(&self) -> State {
        self.state
    }

    fn spy_name(&self) -> Result<String> {
        Ok("rbspy".to_string())
    }

    fn sample_rate(&self) -> Result<u32> {
        Ok(self.config.sample_rate)
    }

    fn initialize(&mut self) -> Result<()> {
        // Check if Backend is Uninitialized
        if self.state != State::Uninitialized {
            return Err(PyroscopeError::new("Rbspy: Backend is already Initialized"));
        }

        // Check if a process ID is set
        if self.config.pid.is_none() {
            return Err(PyroscopeError::new("Rbspy: No Process ID Specified"));
        }

        // Create Sampler
        self.sampler = Some(Sampler::new(
            self.config.pid.unwrap(), // unwrap is safe because of check above
            self.config.sample_rate,
            self.config.lock_process,
            self.config.time_limit,
            self.config.with_subprocesses,
        ));

        // Set State to Ready
        self.state = State::Ready;

        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        // Check if Backend is Ready
        if self.state != State::Ready {
            return Err(PyroscopeError::new("Rbspy: Backend is not Ready"));
        }

        // Channel for Errors generated by the RubySpy Sampler
        let (error_sender, error_receiver): (ErrorSender, ErrorReceiver) = channel();

        // This is provides enough space for 100 threads.
        // It might be a better idea to figure out how many threads are running and determine the
        // size of the channel based on that.
        let queue_size: usize = self.config.sample_rate as usize * 10 * 100;

        // Channel for StackTraces generated by the RubySpy Sampler
        let (stack_sender, stack_receiver): (
            SyncSender<rbspy::StackTrace>,
            Receiver<rbspy::StackTrace>,
        ) = sync_channel(queue_size);

        // Set Error and Stack Receivers
        self.stack_receiver = Some(stack_receiver);
        self.error_receiver = Some(error_receiver);

        // Get the Sampler
        let sampler = self
            .sampler
            .as_ref()
            .ok_or_else(|| PyroscopeError::new("Rbspy: Sampler is not set"))?;

        // Start the Sampler
        sampler
            .start(stack_sender, error_sender)
            .map_err(|e| PyroscopeError::new(&format!("Rbspy: Sampler Error: {}", e)))?;

        // Set State to Running
        self.state = State::Running;

        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        // Check if Backend is Running
        if self.state != State::Running {
            return Err(PyroscopeError::new("Rbspy: Backend is not Running"));
        }

        // Stop Sampler
        self.sampler
            .as_ref()
            .ok_or_else(|| PyroscopeError::new("Rbspy: Sampler is not set"))?
            .stop();

        // Set State to Running
        self.state = State::Ready;

        Ok(())
    }

    fn report(&mut self) -> Result<Vec<u8>> {
        // Check if Backend is Running
        if self.state != State::Running {
            return Err(PyroscopeError::new("Rbspy: Backend is not Running"));
        }

        // Get an Arc reference to the Report Buffer
        let buffer = self.buffer.clone();

        // Send Errors to Log
        let errors = self
            .error_receiver
            .as_ref()
            .ok_or_else(|| PyroscopeError::new("Rbspy: error receiver is not set"))?
            .try_iter();
        for error in errors {
            match error {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Rbspy: Error in Sampler: {}", e);
                }
            }
        }

        // Collect the StackTrace from the receiver
        let stack_trace = self
            .stack_receiver
            .as_ref()
            .ok_or_else(|| PyroscopeError::new("Rbspy: StackTrace receiver is not set"))?
            .try_iter();

        // Iterate over the StackTrace
        for trace in stack_trace {
            // convert StackTrace
            let own_trace: StackTrace = Into::<StackTraceWrapper>::into(trace).into();
            buffer.lock()?.record(own_trace)?;
        }

        let v8: Vec<u8> = buffer.lock()?.to_string().into_bytes();

        buffer.lock()?.clear();

        // Return the writer's buffer
        Ok(v8)
    }
}

struct StackFrameWrapper(StackFrame);

impl From<StackFrameWrapper> for StackFrame {
    fn from(frame: StackFrameWrapper) -> Self {
        frame.0
    }
}

impl From<rbspy::StackFrame> for StackFrameWrapper {
    fn from(frame: rbspy::StackFrame) -> Self {
        StackFrameWrapper(StackFrame {
            module: None,
            name: Some(frame.name),
            filename: Some(frame.relative_path.clone()),
            relative_path: Some(frame.relative_path),
            absolute_path: frame.absolute_path,
            line: Some(frame.lineno),
        })
    }
}

struct StackTraceWrapper(StackTrace);

impl From<StackTraceWrapper> for StackTrace {
    fn from(trace: StackTraceWrapper) -> Self {
        trace.0
    }
}

impl From<rbspy::StackTrace> for StackTraceWrapper {
    fn from(trace: rbspy::StackTrace) -> Self {
        StackTraceWrapper(StackTrace {
            pid: trace.pid.map(|pid| pid as u32),
            thread_id: trace.thread_id.map(|id| id as u64),
            thread_name: None,
            frames: trace
                .iter()
                .map(|frame| Into::<StackFrameWrapper>::into(frame.clone()).into())
                .collect(),
        })
    }
}
