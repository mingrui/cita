// CITA
// Copyright 2016-2017 Cryptape Technologies LLC.

// This program is free software: you can redistribute it
// and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation,
// either version 3 of the License, or (at your option) any
// later version.

// This program is distributed in the hope that it will be
// useful, but WITHOUT ANY WARRANTY; without even the implied
// warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
// PURPOSE. See the GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

pub use byteorder::{BigEndian, ByteOrder};
use core::filters::eth_filter::EthFilter;
use core::libchain::call_request::CallRequest;
pub use core::libchain::chain::*;
use error::ErrorCode;
use jsonrpc_types::rpctypes;
use jsonrpc_types::rpctypes::{Filter as RpcFilter, Log as RpcLog, Receipt as RpcReceipt, CountOrCode, BlockNumber, BlockParamsByNumber, BlockParamsByHash, RpcBlock};
use libproto;
pub use libproto::*;
use libproto::blockchain::Block as ProtobufBlock;
use libproto::consensus::SignedProposeStep;
pub use libproto::request::Request_oneof_req as Request;
use protobuf::{Message, RepeatedField};
use protobuf::core::parse_from_bytes;
use serde_json;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, Receiver};
use std::vec::Vec;
use types::filter::Filter;
use types::ids::BlockId;
use util::Address;
use util::H256;

// TODO: RPC Errors
pub fn chain_result(chain: Arc<Chain>, rx: &Receiver<(String, Vec<u8>)>, ctx_pub: &Sender<(String, Vec<u8>)>) {
    let (key, msg) = rx.recv().unwrap();
    let (cmd_id, origin, content_ext) = parse_msg(msg.as_slice());

    trace!("chain_result call {:?}", key);
    match content_ext {
        MsgClass::REQUEST(mut req) => {
            let mut response = response::Response::new();
            response.set_request_id(req.take_request_id());
            let topic = "chain.rpc".to_string();
            match req.req.unwrap() {
                // TODO: should check the result, parse it first!
                Request::block_number(_) => {
                    // let sys_time = SystemTime::now();
                    let height = chain.get_current_height();
                    response.set_block_number(height);
                }

                Request::block_by_hash(rpc) => {
                    //let rpc: BlockParamsByHash = serde_json::from_str(&rpc);
                    match serde_json::from_str::<BlockParamsByHash>(&rpc) {
                        Ok(param) => {
                            let hash = param.hash;
                            let include_txs = param.include_txs;
                            match chain.block_by_hash(H256::from(hash.as_slice())) {
                                Some(block) => {
                                    let rpc_block = RpcBlock::new(hash, include_txs, block.protobuf().write_to_bytes().unwrap());
                                    serde_json::to_string(&rpc_block).map(|data| response.set_block(data)).map_err(|err| {
                                                                                                                       response.set_code(ErrorCode::query_error());
                                                                                                                       response.set_error_msg(format!("{:?}", err));
                                                                                                                   });
                                }
                                None => {
                                    response.set_none(true)
                                }
                            }
                        }
                        Err(err) => {
                            response.set_block(format!("{:?}", err));
                            response.set_code(submodules::CHAIN as i64);
                        }
                    };
                }

                Request::block_by_height(block_height) => {
                    let block_height: BlockParamsByNumber = serde_json::from_str(&block_height).expect("Invalid param");
                    let include_txs = block_height.include_txs;
                    match chain.block(block_height.block_id.into()) {
                        Some(block) => {
                            let rpc_block = RpcBlock::new(block.hash().to_vec(), include_txs, block.protobuf().write_to_bytes().unwrap());
                            serde_json::to_string(&rpc_block).map(|data| response.set_block(data)).map_err(|err| {
                                                                                                               response.set_code(ErrorCode::query_error());
                                                                                                               response.set_error_msg(format!("{:?}", err));
                                                                                                           });
                        }
                        None => {
                            response.set_none(true);
                        }
                    }
                }

                Request::transaction(hash) => {
                    match chain.full_transaction(H256::from_slice(&hash)) {
                        Some(ts) => {
                            response.set_ts(ts);
                        }
                        None => {
                            response.set_none(true);
                        }
                    }
                }

                Request::transaction_receipt(hash) => {
                    let tx_hash = H256::from_slice(&hash);
                    let receipt = chain.localized_receipt(tx_hash);
                    if let Some(receipt) = receipt {
                        let rpc_receipt: RpcReceipt = receipt.into();
                        let serialized = serde_json::to_string(&rpc_receipt).unwrap();
                        response.set_receipt(serialized);
                    } else {
                        response.set_none(true);
                    }
                }

                Request::call(call) => {
                    trace!("Chainvm Call {:?}", call);
                    serde_json::from_str::<BlockNumber>(&call.height)
                        .map(|block_id| {
                            let call_request = CallRequest::from(call);
                            chain.eth_call(call_request, block_id.into())
                                 .map(|ok| { response.set_call_result(ok); })
                                 .map_err(|err| {
                                              response.set_code(ErrorCode::query_error());
                                              response.set_error_msg(err);
                                          })
                        })
                        .map_err(|err| {
                                     response.set_code(ErrorCode::query_error());
                                     response.set_error_msg(format!("{:?}", err));
                                 });
                }

                Request::filter(encoded) => {
                    trace!("filter: {:?}", encoded);
                    serde_json::from_str::<RpcFilter>(&encoded)
                        .map_err(|err| {
                                     response.set_code(ErrorCode::query_error());
                                     response.set_error_msg(format!("{:?}", err));
                                 })
                        .map(|rpc_filter| {
                                 let filter: Filter = rpc_filter.into();
                                 let logs = chain.get_logs(filter);
                                 let rpc_logs: Vec<RpcLog> = logs.into_iter().map(|x| x.into()).collect();
                                 response.set_logs(serde_json::to_string(&rpc_logs).unwrap());
                             });
                }

                Request::transaction_count(tx_count) => {
                    trace!("transaction count request from jsonrpc {:?}", tx_count);
                    serde_json::from_str::<CountOrCode>(&tx_count)
                        .map_err(|err| {
                                     response.set_code(ErrorCode::query_error());
                                     response.set_error_msg(format!("{:?}", err));
                                 })
                        .map(|tx_count| {
                            let address = Address::from_slice(tx_count.address.as_ref());
                            match chain.nonce(&address, tx_count.block_id.into()) {
                                Some(nonce) => {
                                    response.set_transaction_count(u64::from(nonce));
                                }
                                None => {
                                    response.set_transaction_count(0);
                                }
                            };
                        });
                }

                Request::code(code_content) => {
                    trace!("code request from josnrpc  {:?}", code_content);
                    serde_json::from_str::<CountOrCode>(&code_content)
                        .map_err(|err| {
                                     response.set_code(ErrorCode::query_error());
                                     response.set_error_msg(format!("{:?}", err));
                                 })
                        .map(|code_content| {
                            let address = Address::from_slice(code_content.address.as_ref());
                            match chain.code_at(&address, code_content.block_id.into()) {
                                Some(code) => {
                                    match code {
                                        Some(code) => {
                                            response.set_contract_code(code);
                                        }
                                        None => {
                                            response.set_contract_code(vec![]);
                                        }
                                    }
                                }
                                None => {
                                    response.set_contract_code(vec![]);
                                }
                            };
                        });
                }

                Request::new_filter(new_filter) => {
                    trace!("new_filter {:?}", new_filter);
                    let new_filter: RpcFilter = serde_json::from_str(&new_filter).expect("Invalid param");
                    trace!("new_filter {:?}", new_filter);
                    response.set_filter_id(chain.new_filter(new_filter) as u64);
                }

                Request::new_block_filter(_) => {
                    let block_filter = chain.new_block_filter();
                    response.set_filter_id(block_filter as u64);
                }

                Request::uninstall_filter(filter_id) => {
                    trace!("uninstall_filter's id is {:?}", filter_id);
                    let index = rpctypes::Index(filter_id as usize);
                    let b = chain.uninstall_filter(index);
                    response.set_uninstall_filter(b);
                }

                Request::filter_changes(filter_id) => {
                    trace!("filter_changes's id is {:?}", filter_id);
                    let index = rpctypes::Index(filter_id as usize);
                    let log = chain.filter_changes(index).unwrap();
                    trace!("Log is: {:?}", log);
                    response.set_filter_changes(serde_json::to_string(&log).unwrap());
                }

                Request::filter_logs(filter_id) => {
                    trace!("filter_log's id is {:?}", filter_id);
                    let index = rpctypes::Index(filter_id as usize);
                    let log = chain.filter_logs(index).unwrap_or(vec![]);
                    trace!("Log is: {:?}", log);
                    response.set_filter_logs(serde_json::to_string(&log).unwrap());
                }
                _ => {
                    error!("mtach error Request_oneof_req msg!!!!");
                }
            };
            let msg: communication::Message = response.into();
            ctx_pub.send((topic, msg.write_to_bytes().unwrap())).unwrap();
        }

        MsgClass::BLOCKWITHPROOF(proofblk) => {
            let mut guard = chain.block_map.write();

            let current_height = chain.get_current_height();
            let max_height = chain.get_max_height();
            let block = proofblk.get_blk();
            let proof = proofblk.get_proof();
            let blk_height = block.get_header().get_height();

            let new_map = guard.split_off(&current_height);
            *guard = new_map;

            trace!("received proof block: block_number:{:?} current_height: {:?} max_height: {:?}", blk_height, current_height, max_height);

            if blk_height > current_height && blk_height < current_height + 300 {
                if !guard.contains_key(&blk_height) || (guard.contains_key(&blk_height) && guard[&blk_height].2 == false) {
                    trace!("block insert {:?}", blk_height);
                    guard.insert(blk_height, (Some(proof.clone()), Block::from(block.clone()), true));
                    let _ = chain.sync_sender.lock().send(blk_height);
                }
            }
        }

        MsgClass::BLOCK(problock) => {
            let current_height = chain.get_current_height();
            let max_height = chain.get_max_height();
            let blk_height = problock.get_header().get_height();

            // Check transaction root
            // ignore block which height is ::std::u64::MAX, it's only a proof
            if blk_height != ::std::u64::MAX && !problock.check_hash() {
                warn!("transactions root isn't correct, height is {}", blk_height);
                return;
            }

            let block = Block::from(problock.clone());
            let check_height = Chain::get_block_proof_height(&block);

            if blk_height == ::std::u64::MAX {
                if check_height != ::std::usize::MAX {
                    let proof_height = check_height as u64;
                    let mut guard = chain.block_map.write();
                    if let Some(info) = guard.get_mut(&proof_height) {
                        info.0 = Some(problock.get_header().get_proof().clone());
                        let _ = chain.sync_sender.lock().send(proof_height);
                        trace!("blk_height == MAX proof height {}", proof_height);
                    }
                }
                return;
            }
            let proof_height = check_height as u64;
            trace!("received block: block_number:{:?} current_height: {:?} max_height: {:?} proof_height: {:?}", blk_height, current_height, max_height, proof_height);
            if blk_height > current_height && blk_height < current_height + 300 {
                let min_height = {
                    if proof_height == 0 { current_height } else { ::std::cmp::min(current_height, proof_height) }
                };

                let mut guard = chain.block_map.write();
                let new_map = guard.split_off(&min_height);
                *guard = new_map;
                if !guard.contains_key(&blk_height) {
                    trace!("block insert {:?} no proof and not verified", blk_height);
                    guard.insert(blk_height, (None, block, false));
                }
                if let Some(info) = guard.get_mut(&proof_height) {
                    info.0 = Some(problock.get_header().get_proof().clone());
                    let _ = chain.sync_sender.lock().send(proof_height);
                }
            }
        }

        MsgClass::STATUS(status) => {
            let status_height = status.get_height();
            if status_height > chain.get_max_height() {
                chain.max_height.store(status_height as usize, Ordering::SeqCst);
                trace!("recieved status update max_height: {:?}", status_height);
            }
            let known_max_height = chain.get_max_height();
            let current_height = chain.get_current_height();
            let target_height = ::std::cmp::min(current_height + 100, known_max_height);
            if current_height < target_height && !chain.is_sync.load(Ordering::SeqCst) {
                let mut diff = target_height - current_height;
                let mut start_height = current_height + 1;
                while diff > 0 {
                    let mut wtr = vec![0; 8];
                    trace!("request sync {:?}", start_height);
                    BigEndian::write_u64(&mut wtr, start_height);
                    let msg = factory::create_msg_ex(submodules::CHAIN, topics::SYNC_BLK, communication::MsgType::MSG, communication::OperateType::SINGLE, origin, wtr);
                    trace!("origin {:?}, chain.sync: OperateType {:?}", origin, communication::OperateType::SINGLE);
                    ctx_pub.send(("chain.sync".to_string(), msg.write_to_bytes().unwrap())).unwrap();
                    start_height += 1;
                    diff -= 1;
                }
                if !chain.is_sync.load(Ordering::SeqCst) {
                    chain.is_sync.store(true, Ordering::SeqCst);
                }
            }
        }

        MsgClass::MSG(content) => {
            if libproto::cmd_id(submodules::CHAIN, topics::SYNC_BLK) == cmd_id {

                let height = BigEndian::read_u64(&content);
                trace!("Receive sync {:?} from node-{:?}", height, origin);
                if let Some(block) = chain.block(BlockId::Number(height)) {
                    let msg = factory::create_msg_ex(submodules::CHAIN, topics::NEW_BLK, communication::MsgType::BLOCK, communication::OperateType::SINGLE, origin, block.protobuf().write_to_bytes().unwrap());
                    trace!("origin {:?}, chain.blk: OperateType {:?}", origin, communication::OperateType::SINGLE);
                    ctx_pub.send(("chain.blk".to_string(), msg.write_to_bytes().unwrap())).unwrap();

                    if height == chain.get_current_height() {
                        let mut proof_block = ProtobufBlock::new();
                        let mut flag = false;
                        {
                            let guard = chain.block_map.read();
                            if let Some(&(Some(ref proof), _, _)) = guard.get(&height) {
                                proof_block.mut_header().set_proof(proof.clone());
                                flag = true;
                            }
                        }
                        if flag {
                            proof_block.mut_header().set_height(::std::u64::MAX);
                            let msg = factory::create_msg_ex(submodules::CHAIN, topics::NEW_BLK, communication::MsgType::BLOCK, communication::OperateType::SINGLE, origin, proof_block.write_to_bytes().unwrap());
                            trace!("max height {:?}, chain.blk: OperateType {:?}", height, communication::OperateType::SINGLE);
                            ctx_pub.send(("chain.blk".to_string(), msg.write_to_bytes().unwrap())).unwrap();
                        }
                    }
                }
            } else if libproto::cmd_id(submodules::CONSENSUS, topics::NEW_PROPOSAL) == cmd_id {
                info!("Receive new proposal.");
                let signed_propose_step = parse_from_bytes::<SignedProposeStep>(&content).unwrap();

                let proto_block = signed_propose_step.get_propose_step().get_proposal().get_block();
                trace!("protobuf block is {:?}", proto_block);

            } else {
                trace!("Receive other message content.");
            }
        }

        MsgClass::BLOCKTXHASHESREQ(block_tx_hashes_req) => {
            let block_height = block_tx_hashes_req.get_height();
            if let Some(tx_hashes) = chain.transaction_hashes(BlockId::Number(block_height)) {
                //prepare and send the block tx hashes to auth
                let mut block_tx_hashes = BlockTxHashes::new();
                block_tx_hashes.set_height(block_height);
                let mut tx_hashes_in_u8 = Vec::new();
                for tx_hash_in_h256 in tx_hashes.iter() {
                    tx_hashes_in_u8.push(tx_hash_in_h256.to_vec());
                }
                block_tx_hashes.set_tx_hashes(RepeatedField::from_slice(&tx_hashes_in_u8[..]));
                block_tx_hashes.set_block_gas_limit(chain.block_gas_limit.load(Ordering::SeqCst) as u64);
                block_tx_hashes.set_account_gas_limit(chain.account_gas_limit.read().clone().into());

                let msg = factory::create_msg(submodules::CHAIN, topics::BLOCK_TXHASHES, communication::MsgType::BLOCK_TXHASHES, block_tx_hashes.write_to_bytes().unwrap());

                ctx_pub.send(("chain.txhashes".to_string(), msg.write_to_bytes().unwrap())).unwrap();
                trace!("response block's tx hashes for height:{}", block_height);
            } else {
                warn!("get block's tx hashes for height:{} error", block_height);
            }
        }
        MsgClass::RICHSTATUS(rich_status) => {
            info!("forward dispatch rich_status is {:?}", rich_status);
        }
        _ => {
            error!("error MsgClass!!!!");
        }
    }
}
