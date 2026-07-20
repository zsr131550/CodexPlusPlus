use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};

pub const MANAGER_INSTANCE_SCHEMA: u16 = 1;
pub const MAX_ACTIVATION_FRAME_BYTES: usize = 512;

const ENDPOINT_RECORD_MAX_BYTES: u64 = 1_024;
const RETRY_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerActivation {
    Show,
    ReloadPendingProviderImport,
    ShowUpdate,
}

#[derive(Clone)]
pub struct ManagerInstanceConfig {
    state_dir: PathBuf,
    preferred_port: u16,
    initialization_timeout: Duration,
    io_timeout: Duration,
}

impl ManagerInstanceConfig {
    pub fn for_state_dir(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            preferred_port: crate::ports::manager_guard_port(),
            initialization_timeout: Duration::from_millis(750),
            io_timeout: Duration::from_millis(250),
        }
    }

    pub fn with_preferred_port(mut self, preferred_port: u16) -> Self {
        self.preferred_port = preferred_port;
        self
    }

    pub fn with_initialization_timeout(mut self, timeout: Duration) -> Self {
        self.initialization_timeout = timeout;
        self
    }

    pub fn with_io_timeout(mut self, timeout: Duration) -> Self {
        self.io_timeout = timeout;
        self
    }

    pub fn instance_dir(&self) -> PathBuf {
        self.state_dir.clone()
    }

    pub fn lock_path(&self) -> PathBuf {
        self.instance_dir().join("manager-instance.lock")
    }

    pub fn endpoint_path(&self) -> PathBuf {
        self.instance_dir().join("manager-instance-endpoint.json")
    }
}

impl fmt::Debug for ManagerInstanceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerInstanceConfig")
            .field("preferred_port", &self.preferred_port)
            .field("initialization_timeout", &self.initialization_timeout)
            .field("io_timeout", &self.io_timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerInstanceError {
    #[error("manager instance I/O failed during {0}")]
    Io(&'static str),
    #[error("manager owner endpoint was not published in time")]
    OwnerInitializationTimedOut,
    #[error("manager endpoint record is invalid")]
    InvalidEndpointRecord,
    #[error("manager activation frame is invalid")]
    InvalidActivationFrame,
    #[error("manager activation frame exceeds the size limit")]
    ActivationFrameTooLarge,
    #[error("manager activation timed out")]
    ActivationTimedOut,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagerEndpointRecord {
    schema: u16,
    pid: u32,
    address: SocketAddr,
    nonce: String,
}

impl fmt::Debug for ManagerEndpointRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerEndpointRecord")
            .field("schema", &self.schema)
            .field("pid", &self.pid)
            .field("address", &self.address)
            .field("nonce_present", &!self.nonce.is_empty())
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagerActivationEnvelope {
    schema: u16,
    nonce: String,
    action: ManagerActivation,
}

pub enum ManagerInstance {
    Primary(ManagerInstanceOwner),
    Secondary(ManagerInstanceClient),
}

impl fmt::Debug for ManagerInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary(owner) => formatter.debug_tuple("Primary").field(owner).finish(),
            Self::Secondary(client) => formatter.debug_tuple("Secondary").field(client).finish(),
        }
    }
}

pub struct ManagerInstanceOwner {
    lock_file: File,
    listener: TcpListener,
    endpoint_path: PathBuf,
    nonce: String,
    io_timeout: Duration,
}

impl ManagerInstanceOwner {
    pub fn endpoint_path(&self) -> &Path {
        &self.endpoint_path
    }

    pub fn local_addr(&self) -> Result<SocketAddr, ManagerInstanceError> {
        self.listener
            .local_addr()
            .map_err(|_| ManagerInstanceError::Io("reading owner address"))
    }

    pub fn receiver(&self) -> Result<ManagerActivationReceiver, ManagerInstanceError> {
        let listener = self
            .listener
            .try_clone()
            .map_err(|_| ManagerInstanceError::Io("cloning owner listener"))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| ManagerInstanceError::Io("configuring activation listener"))?;
        Ok(ManagerActivationReceiver {
            listener,
            nonce: self.nonce.clone(),
            io_timeout: self.io_timeout,
        })
    }
}

impl fmt::Debug for ManagerInstanceOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerInstanceOwner")
            .field("address", &self.listener.local_addr().ok())
            .field("nonce_present", &!self.nonce.is_empty())
            .finish_non_exhaustive()
    }
}

impl Drop for ManagerInstanceOwner {
    fn drop(&mut self) {
        if read_endpoint_record(&self.endpoint_path).is_ok_and(|record| record.nonce == self.nonce)
        {
            let _ = std::fs::remove_file(&self.endpoint_path);
        }
        let _ = fs2::FileExt::unlock(&self.lock_file);
    }
}

#[derive(Clone)]
pub struct ManagerInstanceClient {
    address: SocketAddr,
    nonce: String,
    io_timeout: Duration,
}

impl ManagerInstanceClient {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn send(&self, action: ManagerActivation) -> Result<(), ManagerInstanceError> {
        let bytes = serde_json::to_vec(&ManagerActivationEnvelope {
            schema: MANAGER_INSTANCE_SCHEMA,
            nonce: self.nonce.clone(),
            action,
        })
        .map_err(|_| ManagerInstanceError::InvalidActivationFrame)?;
        if bytes.len() > MAX_ACTIVATION_FRAME_BYTES {
            return Err(ManagerInstanceError::ActivationFrameTooLarge);
        }

        let mut stream = TcpStream::connect_timeout(&self.address, self.io_timeout)
            .map_err(|_| ManagerInstanceError::Io("connecting to owner"))?;
        stream
            .set_write_timeout(Some(self.io_timeout))
            .map_err(|_| ManagerInstanceError::Io("configuring activation write"))?;
        stream
            .write_all(&bytes)
            .map_err(|_| ManagerInstanceError::Io("sending activation"))?;
        stream
            .shutdown(Shutdown::Write)
            .map_err(|_| ManagerInstanceError::Io("finishing activation"))?;
        Ok(())
    }
}

impl fmt::Debug for ManagerInstanceClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerInstanceClient")
            .field("address", &self.address)
            .field("nonce_present", &!self.nonce.is_empty())
            .finish()
    }
}

pub struct ManagerActivationReceiver {
    listener: TcpListener,
    nonce: String,
    io_timeout: Duration,
}

impl ManagerActivationReceiver {
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<ManagerActivation, ManagerInstanceError> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Ok(action) = read_activation(stream, &self.nonce, self.io_timeout) {
                        return Ok(action);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(ManagerInstanceError::Io("accepting activation")),
            }

            if Instant::now() >= deadline {
                return Err(ManagerInstanceError::ActivationTimedOut);
            }
            thread::sleep(RETRY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
        }
    }
}

impl fmt::Debug for ManagerActivationReceiver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerActivationReceiver")
            .field("address", &self.listener.local_addr().ok())
            .field("nonce_present", &!self.nonce.is_empty())
            .finish()
    }
}

pub fn acquire_manager_instance(
    config: ManagerInstanceConfig,
) -> Result<ManagerInstance, ManagerInstanceError> {
    std::fs::create_dir_all(config.instance_dir())
        .map_err(|_| ManagerInstanceError::Io("creating instance directory"))?;
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.lock_path())
        .map_err(|_| ManagerInstanceError::Io("opening owner lock"))?;

    match lock_file.try_lock_exclusive() {
        Ok(()) => acquire_primary(config, lock_file),
        Err(error) if is_lock_conflict(&error) => wait_for_secondary(config),
        Err(_) => Err(ManagerInstanceError::Io("acquiring owner lock")),
    }
}

fn acquire_primary(
    config: ManagerInstanceConfig,
    lock_file: File,
) -> Result<ManagerInstance, ManagerInstanceError> {
    match std::fs::remove_file(config.endpoint_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ManagerInstanceError::Io("clearing stale endpoint")),
    }

    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, config.preferred_port)) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .map_err(|_| ManagerInstanceError::Io("binding fallback endpoint"))?
        }
        Err(_) => return Err(ManagerInstanceError::Io("binding preferred endpoint")),
    };
    listener
        .set_nonblocking(true)
        .map_err(|_| ManagerInstanceError::Io("configuring owner listener"))?;
    let address = listener
        .local_addr()
        .map_err(|_| ManagerInstanceError::Io("reading owner address"))?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let record = ManagerEndpointRecord {
        schema: MANAGER_INSTANCE_SCHEMA,
        pid: std::process::id(),
        address,
        nonce: nonce.clone(),
    };
    write_endpoint_record(config.endpoint_path(), &record)?;

    Ok(ManagerInstance::Primary(ManagerInstanceOwner {
        lock_file,
        listener,
        endpoint_path: config.endpoint_path(),
        nonce,
        io_timeout: config.io_timeout,
    }))
}

fn wait_for_secondary(
    config: ManagerInstanceConfig,
) -> Result<ManagerInstance, ManagerInstanceError> {
    let deadline = Instant::now() + config.initialization_timeout;
    loop {
        if let Ok(record) = read_endpoint_record(config.endpoint_path())
            && endpoint_is_live(record.address, config.io_timeout)
        {
            return Ok(ManagerInstance::Secondary(ManagerInstanceClient {
                address: record.address,
                nonce: record.nonce,
                io_timeout: config.io_timeout,
            }));
        }
        if Instant::now() >= deadline {
            return Err(ManagerInstanceError::OwnerInitializationTimedOut);
        }
        thread::sleep(RETRY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn endpoint_is_live(address: SocketAddr, timeout: Duration) -> bool {
    TcpStream::connect_timeout(&address, timeout)
        .and_then(|stream| stream.shutdown(Shutdown::Both))
        .is_ok()
}

fn is_lock_conflict(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock || error.raw_os_error() == Some(33)
}

fn read_endpoint_record(
    path: impl AsRef<Path>,
) -> Result<ManagerEndpointRecord, ManagerInstanceError> {
    let file =
        File::open(path.as_ref()).map_err(|_| ManagerInstanceError::InvalidEndpointRecord)?;
    if file
        .metadata()
        .map_err(|_| ManagerInstanceError::InvalidEndpointRecord)?
        .len()
        > ENDPOINT_RECORD_MAX_BYTES
    {
        return Err(ManagerInstanceError::InvalidEndpointRecord);
    }
    let record: ManagerEndpointRecord =
        serde_json::from_reader(file).map_err(|_| ManagerInstanceError::InvalidEndpointRecord)?;
    if record.schema != MANAGER_INSTANCE_SCHEMA
        || record.pid == 0
        || !record.address.ip().is_loopback()
        || record.nonce.is_empty()
        || record.nonce.len() > 128
    {
        return Err(ManagerInstanceError::InvalidEndpointRecord);
    }
    Ok(record)
}

fn write_endpoint_record(
    path: impl AsRef<Path>,
    record: &ManagerEndpointRecord,
) -> Result<(), ManagerInstanceError> {
    let mut bytes =
        serde_json::to_vec(record).map_err(|_| ManagerInstanceError::InvalidEndpointRecord)?;
    bytes.push(b'\n');
    crate::settings::atomic_write(path.as_ref(), &bytes)
        .map_err(|_| ManagerInstanceError::Io("publishing owner endpoint"))
}

fn read_activation(
    mut stream: TcpStream,
    nonce: &str,
    timeout: Duration,
) -> Result<ManagerActivation, ManagerInstanceError> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|_| ManagerInstanceError::Io("configuring activation read"))?;
    let mut bytes = Vec::with_capacity(MAX_ACTIVATION_FRAME_BYTES);
    Read::by_ref(&mut stream)
        .take(MAX_ACTIVATION_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ManagerInstanceError::InvalidActivationFrame)?;
    if bytes.len() > MAX_ACTIVATION_FRAME_BYTES {
        return Err(ManagerInstanceError::ActivationFrameTooLarge);
    }
    let envelope: ManagerActivationEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| ManagerInstanceError::InvalidActivationFrame)?;
    if envelope.schema != MANAGER_INSTANCE_SCHEMA || envelope.nonce != nonce {
        return Err(ManagerInstanceError::InvalidActivationFrame);
    }
    Ok(envelope.action)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::time::Duration;

    use fs2::FileExt as _;

    use super::*;

    fn config(directory: &std::path::Path, preferred_port: u16) -> ManagerInstanceConfig {
        ManagerInstanceConfig::for_state_dir(directory)
            .with_preferred_port(preferred_port)
            .with_initialization_timeout(Duration::from_millis(350))
            .with_io_timeout(Duration::from_millis(100))
    }

    fn available_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn primary(instance: ManagerInstance) -> ManagerInstanceOwner {
        match instance {
            ManagerInstance::Primary(owner) => owner,
            ManagerInstance::Secondary(_) => panic!("expected primary manager instance"),
        }
    }

    fn secondary(instance: ManagerInstance) -> ManagerInstanceClient {
        match instance {
            ManagerInstance::Secondary(client) => client,
            ManagerInstance::Primary(_) => panic!("expected secondary manager instance"),
        }
    }

    #[test]
    fn manager_instance_primary_publishes_a_live_authenticated_endpoint() {
        let temp = tempfile::tempdir().unwrap();
        let owner =
            primary(acquire_manager_instance(config(temp.path(), available_port())).unwrap());
        let record = read_endpoint_record(owner.endpoint_path()).unwrap();

        assert_eq!(record.schema, MANAGER_INSTANCE_SCHEMA);
        assert_eq!(record.pid, std::process::id());
        assert!(record.address.ip().is_loopback());
        assert_eq!(record.address, owner.local_addr().unwrap());
        assert!(!record.nonce.is_empty());
        assert!(!format!("{owner:?}").contains(&record.nonce));
    }

    #[test]
    fn manager_instance_secondary_sends_typed_actions_to_the_owner() {
        let temp = tempfile::tempdir().unwrap();
        let owner =
            primary(acquire_manager_instance(config(temp.path(), available_port())).unwrap());
        let client = secondary(
            acquire_manager_instance(config(temp.path(), owner.local_addr().unwrap().port()))
                .unwrap(),
        );

        client
            .send(ManagerActivation::ReloadPendingProviderImport)
            .unwrap();

        assert_eq!(
            owner
                .receiver()
                .unwrap()
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ManagerActivation::ReloadPendingProviderImport
        );
    }

    #[test]
    fn manager_instance_port_collision_falls_back_to_an_ephemeral_live_listener() {
        let temp = tempfile::tempdir().unwrap();
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let preferred = occupied.local_addr().unwrap().port();

        let owner = primary(acquire_manager_instance(config(temp.path(), preferred)).unwrap());
        let client = secondary(acquire_manager_instance(config(temp.path(), preferred)).unwrap());
        client.send(ManagerActivation::Show).unwrap();

        assert_ne!(owner.local_addr().unwrap().port(), preferred);
        assert_eq!(
            owner
                .receiver()
                .unwrap()
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ManagerActivation::Show
        );
    }

    #[test]
    fn manager_instance_secondary_waits_for_owner_endpoint_publication() {
        let temp = tempfile::tempdir().unwrap();
        let config = config(temp.path(), available_port());
        std::fs::create_dir_all(config.instance_dir()).unwrap();
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(config.lock_path())
            .unwrap();
        lock.try_lock_exclusive().unwrap();

        let waiting_config = config.clone();
        let waiter = std::thread::spawn(move || acquire_manager_instance(waiting_config));
        std::thread::sleep(Duration::from_millis(40));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let record = ManagerEndpointRecord {
            schema: MANAGER_INSTANCE_SCHEMA,
            pid: std::process::id(),
            address: listener.local_addr().unwrap(),
            nonce: "owner-initialization-nonce".to_owned(),
        };
        write_endpoint_record(config.endpoint_path(), &record).unwrap();

        let client = secondary(waiter.join().unwrap().unwrap());
        assert_eq!(client.address(), record.address);
    }

    #[test]
    fn manager_instance_stale_or_missing_endpoint_times_out_without_stealing_ownership() {
        let temp = tempfile::tempdir().unwrap();
        let config = config(temp.path(), available_port())
            .with_initialization_timeout(Duration::from_millis(40));
        std::fs::create_dir_all(config.instance_dir()).unwrap();
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(config.lock_path())
            .unwrap();
        lock.try_lock_exclusive().unwrap();
        std::fs::write(config.endpoint_path(), b"not-json").unwrap();

        let error = acquire_manager_instance(config).unwrap_err();

        assert!(matches!(
            error,
            ManagerInstanceError::OwnerInitializationTimedOut
        ));
    }

    #[test]
    fn manager_instance_rejects_wrong_nonce_schema_partial_trailing_and_oversized_frames() {
        let temp = tempfile::tempdir().unwrap();
        let owner =
            primary(acquire_manager_instance(config(temp.path(), available_port())).unwrap());
        let record = read_endpoint_record(owner.endpoint_path()).unwrap();
        let receiver = owner.receiver().unwrap();

        for bytes in [
            envelope_bytes(MANAGER_INSTANCE_SCHEMA, "wrong", ManagerActivation::Show),
            envelope_bytes(
                MANAGER_INSTANCE_SCHEMA + 1,
                &record.nonce,
                ManagerActivation::Show,
            ),
            serde_json::to_vec(&serde_json::json!({
                "schema": MANAGER_INSTANCE_SCHEMA,
                "nonce": &record.nonce,
                "action": ManagerActivation::Show,
                "payload": "payloads-are-not-allowed",
            }))
            .unwrap(),
            b"{\"schema\":".to_vec(),
            [
                envelope_bytes(
                    MANAGER_INSTANCE_SCHEMA,
                    &record.nonce,
                    ManagerActivation::Show,
                ),
                b" trailing".to_vec(),
            ]
            .concat(),
            vec![b'x'; MAX_ACTIVATION_FRAME_BYTES + 1],
        ] {
            send_raw(record.address, &bytes);
            assert!(receiver.recv_timeout(Duration::from_secs(1)).is_err());
        }
    }

    #[test]
    fn manager_instance_owner_drop_removes_record_and_releases_lock() {
        let temp = tempfile::tempdir().unwrap();
        let config = config(temp.path(), available_port());
        let owner = primary(acquire_manager_instance(config.clone()).unwrap());
        let endpoint_path = owner.endpoint_path().to_path_buf();
        drop(owner);

        assert!(!endpoint_path.exists());
        assert!(matches!(
            acquire_manager_instance(config).unwrap(),
            ManagerInstance::Primary(_)
        ));
    }

    fn envelope_bytes(schema: u16, nonce: &str, action: ManagerActivation) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": schema,
            "nonce": nonce,
            "action": action,
        }))
        .unwrap()
    }

    fn send_raw(address: SocketAddr, bytes: &[u8]) {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(bytes).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
    }
}
