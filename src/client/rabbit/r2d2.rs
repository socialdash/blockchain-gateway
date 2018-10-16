use std::io::Error as IoError;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::error::*;
use failure::Compat;
use lapin_futures::channel::Channel;
use lapin_futures::client::{Client, ConnectionOptions, HeartbeatHandle};
use prelude::*;
use r2d2::ManageConnection;
use tokio;
use tokio::net::tcp::TcpStream;
use tokio::timer::timeout::Timeout;

pub struct RabbitConnectionManager {
    client: Arc<Mutex<Client<TcpStream>>>,
    heartbeat_handle: Arc<Mutex<HeartbeatHandle>>,
    connection_timeout: Duration,
    address: SocketAddr,
}

impl Drop for RabbitConnectionManager {
    fn drop(&mut self) {
        let handle = self.heartbeat_handle.lock().unwrap();
        handle.stop();
    }
}

impl RabbitConnectionManager {
    pub fn establish(address: SocketAddr, connection_timeout: Duration) -> impl Future<Item = Self, Error = Error> {
        let address_clone = address.clone();
        Timeout::new(
            RabbitConnectionManager::establish_client(address).map(move |(client, hearbeat_handle)| RabbitConnectionManager {
                client: Arc::new(Mutex::new(client)),
                heartbeat_handle: Arc::new(Mutex::new(hearbeat_handle)),
                connection_timeout,
                address,
            }),
            connection_timeout,
        ).map_err(ectx!(ErrorSource::Timeout, ErrorContext::ConnectionTimeout, ErrorKind::Internal => address_clone, connection_timeout))
    }

    fn establish_client(address: SocketAddr) -> impl Future<Item = (Client<TcpStream>, HeartbeatHandle), Error = Error> {
        let address_clone = address.clone();
        let address_clone2 = address.clone();
        let address_clone3 = address.clone();
        TcpStream::connect(&address)
            .map_err(ectx!(ErrorSource::Io, ErrorContext::TcpConnection, ErrorKind::Internal => address_clone3))
            .and_then(move |stream| {
                Client::connect(
                    stream,
                    ConnectionOptions {
                        frame_max: 65535,
                        ..Default::default()
                    },
                ).map_err(ectx!(ErrorSource::Io, ErrorContext::RabbitConnection, ErrorKind::Internal => address_clone2))
            }).and_then(move |(client, heartbeat)| {
                tokio::spawn(heartbeat.map_err(|e| error!("{:?}", e)));
                heartbeat
                    .handle()
                    .ok_or(ectx!(err ErrorContext::HeartbeatHandle, ErrorKind::Internal))
                    .map(move |handle| (client, handle))
            })
    }
}

impl ManageConnection for RabbitConnectionManager {
    type Connection = Channel<TcpStream>;
    type Error = Compat<Error>;
    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        panic!("Not supposed")
    }
    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        unimplemented!()
    }
    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        unimplemented!()
    }
}
