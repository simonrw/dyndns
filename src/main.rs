use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use eyre::Result;
use tokio::{
    net::UdpSocket,
    sync::{
        mpsc::{self, Receiver},
        RwLock,
    },
};
use trust_dns_server::{
    authority::{Authority, LookupObject, LookupOptions, MessageResponseBuilder},
    proto::{
        op::{Header, MessageType, OpCode},
        rr::{RData, Record},
    },
    resolver::{config::NameServerConfigGroup, IntoName},
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    store::{
        forwarder::{ForwardAuthority, ForwardConfig},
        in_memory::InMemoryAuthority,
    },
    ServerFuture,
};

#[derive(Debug)]
enum Instruction {
    Add,
}

struct Handler {
    forward: ForwardAuthority,
    in_memory: Arc<RwLock<InMemoryAuthority>>,
}

#[async_trait]
impl RequestHandler for Handler {
    async fn handle_request<R>(
        &self,
        request: &trust_dns_server::server::Request,
        response_handle: R,
    ) -> ResponseInfo
    where
        R: trust_dns_server::server::ResponseHandler,
    {
        match self.do_handle_request(request, response_handle).await {
            Ok(info) => info,
            Err(error) => {
                tracing::error!(?error, "error in request handler");
                let mut header = Header::new();
                header.set_response_code(trust_dns_server::proto::op::ResponseCode::ServFail);
                header.into()
            }
        }
    }
}

impl Handler {
    pub async fn new(mut update_channel: Receiver<Instruction>) -> Result<Self> {
        let name_server = NameServerConfigGroup::cloudflare();

        // set up the in-memory store
        let in_memory = Arc::new(RwLock::new(InMemoryAuthority::empty(
            ".".into_name().unwrap(),
            trust_dns_server::authority::ZoneType::Primary,
            false,
        )));

        // set up the forwarder
        let config = ForwardConfig {
            name_servers: name_server,
            options: None,
        };
        let authority = ForwardAuthority::try_from_config(
            ".".into_name().unwrap(),
            trust_dns_server::authority::ZoneType::Hint,
            &config,
        )
        .unwrap();

        // spawn listener for update events
        let update_authority = in_memory.clone();
        tokio::spawn(async move {
            tracing::debug!("spawning task to watch for updates");
            while let Some(msg) = update_channel.recv().await {
                tracing::debug!(?msg, "Received update message");

                let in_memory = update_authority.write().await;

                in_memory
                    .upsert(
                        Record::from_rdata(
                            "foobar.com.".into_name().unwrap(),
                            60,
                            RData::A("127.0.0.1".parse().unwrap()),
                        ),
                        10101,
                    )
                    .await;
            }
        });

        Ok(Self {
            in_memory,
            forward: authority,
        })
    }

    async fn do_handle_request<R>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> Result<ResponseInfo>
    where
        R: ResponseHandler,
    {
        if request.op_code() != OpCode::Query {
            eyre::bail!("only queries supported");
        }

        if request.message_type() != MessageType::Query {
            eyre::bail!("only queries supported");
        }

        let lookup_options = LookupOptions::default();

        // try looking up in the in-memory authority
        if let Ok(info) = self
            .in_memory
            .read()
            .await
            .lookup(
                request.query().name(),
                request.query().query_type(),
                lookup_options,
            )
            .await
        {
            let builder = MessageResponseBuilder::from_message_request(request);
            let header = Header::response_from_request(request.header());
            let response = builder.build(header, info.iter(), &[], &[], &[]);
            return Ok(response_handle.send_response(response).await?);
        } else {
            tracing::warn!("domain not resolved by in-memory store");
        }

        // fall back to forwarding resolver
        let info = self
            .forward
            .lookup(
                request.query().name(),
                request.query().query_type(),
                lookup_options,
            )
            .await?;

        let builder = MessageResponseBuilder::from_message_request(request);
        let header = Header::response_from_request(request.header());
        let response = builder.build(header, info.iter(), &[], &[], &[]);
        Ok(response_handle.send_response(response).await?)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let (tx, rx) = mpsc::channel(10);

    let handler = Handler::new(rx).await?;

    let mut server = ServerFuture::new(handler);
    server.register_socket(UdpSocket::bind("127.0.0.1:5300").await?);

    tokio::spawn(async move {
        tracing::debug!("waiting for 5 seconds to send instruction");
        tokio::time::sleep(Duration::from_secs(5)).await;

        match tx.send(Instruction::Add).await {
            Ok(_) => tracing::debug!("instruction sent"),
            Err(e) => tracing::error!(?e, "sending instruction"),
        }
    });

    server.block_until_done().await?;

    Ok(())
}
