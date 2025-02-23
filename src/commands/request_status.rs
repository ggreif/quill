use crate::lib::sign::sign_transport::{SignReplicaV2Transport, SignedMessageWithRequestId};
use crate::lib::IC_URL;
use crate::lib::{get_agent, get_idl_string, sign::signed_message::RequestStatus, AnyhowResult};
use anyhow::{anyhow, Context};
use ic_agent::agent::{Replied, RequestStatusResponse};
use ic_agent::{AgentError, RequestId};
use ic_types::Principal;
use std::convert::TryInto;
use std::str::FromStr;
use std::sync::Arc;

pub async fn sign(
    pem: &Option<String>,
    request_id: RequestId,
    canister_id: Principal,
) -> AnyhowResult<RequestStatus> {
    let mut agent = get_agent(pem)?;
    let transport = SignReplicaV2Transport::new(Some(request_id));
    let data = transport.data.clone();
    agent.set_transport(transport);
    match agent.request_status_raw(&request_id, canister_id).await {
        Err(AgentError::MissingReplicaTransport()) => {
            let message_with_id: SignedMessageWithRequestId =
                data.read().unwrap().clone().try_into()?;
            Ok(message_with_id.message.try_into()?)
        }
        val => panic!("Unexpected output from the signing agent: {:?}", val),
    }
}

pub async fn submit(
    pem: &Option<String>,
    req: &RequestStatus,
    method_name: Option<String>,
) -> AnyhowResult<String> {
    let canister_id = Principal::from_text(&req.canister_id).expect("Couldn't parse canister id");
    let request_id =
        RequestId::from_str(&req.request_id).context("Invalid argument: request_id")?;
    let mut agent = get_agent(pem)?;
    agent.set_transport(ProxySignReplicaV2Transport {
        req: req.clone(),
        http_transport: Arc::new(
            ic_agent::agent::http_transport::ReqwestHttpReplicaV2Transport::create(
                IC_URL.to_string(),
            )
            .unwrap(),
        ),
    });
    let Replied::CallReplied(blob) = async {
        loop {
            match agent.request_status_raw(&request_id, canister_id).await? {
                RequestStatusResponse::Replied { reply } => return Ok(reply),
                RequestStatusResponse::Rejected {
                    reject_code,
                    reject_message,
                } => {
                    return Err(anyhow!(AgentError::ReplicaError {
                        reject_code,
                        reject_message,
                    }))
                }
                RequestStatusResponse::Unknown
                | RequestStatusResponse::Received
                | RequestStatusResponse::Processing => {
                    println!("The request is being processed...");
                }
                RequestStatusResponse::Done => {
                    return Err(anyhow!(AgentError::RequestStatusDoneNoReply(String::from(
                        request_id
                    ),)))
                }
            };

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    .await?;
    get_idl_string(&blob, canister_id, &method_name.unwrap_or_default(), "rets")
        .context("Invalid IDL blob.")
}

pub(crate) struct ProxySignReplicaV2Transport {
    req: RequestStatus,
    http_transport: Arc<dyn 'static + ReplicaV2Transport + Send + Sync>,
}

use ic_agent::agent::ReplicaV2Transport;
use std::future::Future;
use std::pin::Pin;

impl ReplicaV2Transport for ProxySignReplicaV2Transport {
    fn read_state<'a>(
        &'a self,
        _canister_id: Principal,
        _content: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, AgentError>> + Send + 'a>> {
        self.http_transport.read_state(
            Principal::from_text(self.req.canister_id.clone()).unwrap(),
            hex::decode(self.req.content.clone()).unwrap(),
        )
    }

    fn call<'a>(
        &'a self,
        _effective_canister_id: Principal,
        _envelope: Vec<u8>,
        _request_id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<(), AgentError>> + Send + 'a>> {
        unimplemented!()
    }

    fn query<'a>(
        &'a self,
        _effective_canister_id: Principal,
        _envelope: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, AgentError>> + Send + 'a>> {
        unimplemented!()
    }

    fn status<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, AgentError>> + Send + 'a>> {
        unimplemented!()
    }
}
