use coreum_wasm_sdk::assetft::{
    self, BalanceResponse, FrozenBalanceResponse, FrozenBalancesResponse, Query, TokenResponse,
    TokensResponse, WhitelistedBalanceResponse, WhitelistedBalancesResponse,
};
use coreum_wasm_sdk::assetnft::{
    self, BurntNFTResponse, BurntNFTsInClassResponse, ClassResponse, ClassesResponse,
    FrozenResponse, ParamsResponse, WhitelistedAccountsForNFTResponse, WhitelistedResponse,
};

use crate::customResponse::GetInfoResponse;
use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{
    CLASS_ID, CURRENT_TOKEN_ID, DENOM, IS_WHITELISTED, MAX_TOTAL_MINT, MINT_PRICE, NFTS,
    PREREVEAL_TOKEN_URI, PROTOCOL_ADDRESS, PROTOCOL_FEE, SALE_END_TIME, SALE_START_TIME,
    TREASURY_ADDRESS, URI_STATUS,
};
use coreum_wasm_sdk::core::{CoreumMsg, CoreumQueries, CoreumResult};
use coreum_wasm_sdk::nft;
use coreum_wasm_sdk::pagination::PageRequest;
use cosmwasm_std::{
    coin, entry_point, to_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, QueryRequest,
    Response, StdResult, SubMsg, Uint256,
};
use cw2::set_contract_version;
use cw_ownable::{assert_owner, initialize_owner};
// version info for migration info
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ********** Instantiate **********

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> CoreumResult<ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    initialize_owner(deps.storage, deps.api, Some(info.sender.as_ref()))?;

    let issue_msg = CoreumMsg::AssetNFT(assetnft::Msg::IssueClass {
        name: msg.name,
        symbol: msg.symbol.clone(),
        description: msg.description,
        uri: msg.uri,
        uri_hash: msg.uri_hash,
        data: msg.data,
        features: msg.features,
        royalty_rate: msg.royalty_rate,
    });

    let prereveal_token_uri = msg.prereveal_token_uri;
    let treasury_address = msg.treasury_address;
    let protocol_address = msg.protocol_address;
    let current_token_id = msg.current_token_id;
    let mint_price = msg.mint_price;
    let sale_start_time = msg.sale_start_time;
    let sale_end_time = msg.sale_end_time;
    let protocol_fee = msg.protocol_fee;
    let max_total_mint = msg.max_total_mint;
    let uri_status = msg.uri_status;
    let class_id = format!("{}-{}", msg.symbol, env.contract.address).to_lowercase();

    CLASS_ID.save(deps.storage, &class_id)?;
    PREREVEAL_TOKEN_URI.save(deps.storage, &prereveal_token_uri)?;
    TREASURY_ADDRESS.save(deps.storage, &treasury_address)?;
    PROTOCOL_ADDRESS.save(deps.storage, &protocol_address)?;
    CURRENT_TOKEN_ID.save(deps.storage, &current_token_id)?;
    MINT_PRICE.save(deps.storage, &mint_price)?;
    SALE_START_TIME.save(deps.storage, &sale_start_time)?;
    SALE_END_TIME.save(deps.storage, &sale_end_time)?;
    PROTOCOL_FEE.save(deps.storage, &protocol_fee)?;
    MAX_TOTAL_MINT.save(deps.storage, &max_total_mint)?;
    URI_STATUS.save(deps.storage, &uri_status)?;

    Ok(Response::new()
        .add_attribute("owner", info.sender)
        .add_attribute("class_id", class_id)
        .add_message(issue_msg))
}

// ********** Execute **********

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> CoreumResult<ContractError> {
    match msg {
        ExecuteMsg::Mint {
            id,
            uri,
            uri_hash,
            data,
        } => mint(deps, info, id, uri, uri_hash, data),
        ExecuteMsg::Burn { id } => burn(deps, info, id),
        ExecuteMsg::Freeze { id } => freeze(deps, info, id),
        ExecuteMsg::Unfreeze { id } => unfreeze(deps, info, id),
        ExecuteMsg::AddToWhitelist { id, account } => add_to_white_list(deps, info, id, account),
        ExecuteMsg::RemoveFromWhitelist { id, account } => {
            remove_from_white_list(deps, info, id, account)
        }
        ExecuteMsg::Send { id, receiver } => send(deps, info, id, receiver),
        ExecuteMsg::Purchase {
            count,
            id,
            uri,
            uri_hash,
            data,
            receiver,
        } => purchase(deps, info, id, count, uri, uri_hash, data, receiver),

        ExecuteMsg::MintAndSend { account, amount } => mint_and_send(&deps, &info, account, amount),
    }
}

// ********** Transactions **********
fn mint_and_send(
    deps: &DepsMut,
    info: &MessageInfo,
    account: String,
    amount: u128,
) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let denom = DENOM.load(deps.storage)?;

    let mint_msg = SubMsg::new(CoreumMsg::AssetFT(assetft::Msg::Mint {
        coin: coin(amount, denom.clone()),
    }));

    let send_msg = SubMsg::new(cosmwasm_std::BankMsg::Send {
        to_address: account,
        amount: vec![Coin {
            amount: amount.into(),
            denom: denom.clone(),
        }],
    });

    Ok(Response::new()
        .add_attribute("method", "mint_and_send")
        .add_attribute("denom", denom)
        .add_attribute("amount", amount.to_string())
        .add_submessages([mint_msg, send_msg]))
}
fn purchase(
    deps: DepsMut,
    info: MessageInfo,
    id: String,
    count: Uint256,
    uri: Option<String>,
    uri_hash: Option<String>,
    data: Option<Binary>,
    receiver: String,
) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let owner = info.sender.clone().to_string();
    let permission = IS_WHITELISTED.load(deps.storage, &owner)?;
    let class_id = CLASS_ID.load(deps.storage)?;
    let current_token_id = CURRENT_TOKEN_ID.load(deps.storage)?;
    let max_total_mint = MAX_TOTAL_MINT.load(deps.storage)?;
    let mint_price = MINT_PRICE.load(deps.storage)?;
    let protocol_address = PROTOCOL_ADDRESS.load(deps.storage)?;
    let treasury_address = TREASURY_ADDRESS.load(deps.storage)?;
    let my_integer_0: u32 = 0;
    let zero = Uint256::from(my_integer_0);
    let my_integer_100: u32 = 100;
    let hundred = Uint256::from(my_integer_100);
    assert_eq!(permission, true, "You are not whitelisted");

    if count <= zero {
        panic!("Count must be greater than zero");
    };

    if (current_token_id + count) > max_total_mint {
        panic!("Max total mint exceeded");
    };

    let funds = info
        .funds
        .iter()
        .find(|coin| coin.denom == "uscrt")
        .map(|coin| Uint256::from(coin.amount))
        .unwrap_or_else(Uint256::zero);
    if funds < mint_price * count {
        panic!("Failed to purchase");
    };

    let address_protocol = String::from(protocol_address);

    let protocol_fee_amount: Uint256 =
        funds.checked_mul(PROTOCOL_FEE.load(deps.storage)?).unwrap() / hundred;
    let protocol_fees_arr: [u8; 32] = protocol_fee_amount.to_be_bytes();
    let mut result1: u128 = 0;
    for i in 0..16 {
        result1 <<= 8; // Shift the current value left by 8 bits
        result1 |= protocol_fees_arr[i] as u128; // Bitwise OR with the current byte
    }
    let protocol_fee_amount = result1;
    let funds_arr: [u8; 32] = funds.to_be_bytes();
    let mut result: u128 = 0;
    for i in 0..16 {
        result <<= 8; // Shift the current value left by 8 bits
        result |= funds_arr[i] as u128; // Bitwise OR with the current byte
    }
    let amount = result;
    let treasury_amount = amount - protocol_fee_amount;
    let address_treasury = String::from(treasury_address);

    let protocol_fee_send_response = mint_and_send(&deps, &info, address_protocol, protocol_fee_amount);

    let treasury_fee_send_response = mint_and_send(&deps, &info, address_treasury, treasury_amount);

    let msg_mint = CoreumMsg::AssetNFT(assetnft::Msg::Mint {
        class_id: class_id.clone(),
        id: id.clone(),
        uri,
        uri_hash,
        data,
    });

    let msg_send = CoreumMsg::NFT(nft::Msg::Send {
        class_id: class_id.clone(),
        id: id.clone(),
        receiver,
    });

    Ok(Response::new()
        .add_attribute("method", "mint")
        .add_attribute("method", "send")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg_mint)
        .add_message(msg_send))
}

fn mint(
    deps: DepsMut,
    info: MessageInfo,
    id: String,
    uri: Option<String>,
    uri_hash: Option<String>,
    data: Option<Binary>,
) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;

    let msg = CoreumMsg::AssetNFT(assetnft::Msg::Mint {
        class_id: class_id.clone(),
        id: id.clone(),
        uri,
        uri_hash,
        data,
    });

    Ok(Response::new()
        .add_attribute("method", "mint")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

fn burn(deps: DepsMut, info: MessageInfo, id: String) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;

    let msg = CoreumMsg::AssetNFT(assetnft::Msg::Burn {
        class_id: class_id.clone(),
        id: id.clone(),
    });

    Ok(Response::new()
        .add_attribute("method", "burn")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

fn freeze(deps: DepsMut, info: MessageInfo, id: String) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;

    let msg = CoreumMsg::AssetNFT(assetnft::Msg::Freeze {
        class_id: class_id.clone(),
        id: id.clone(),
    });

    Ok(Response::new()
        .add_attribute("method", "freeze")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

fn unfreeze(deps: DepsMut, info: MessageInfo, id: String) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;

    let msg = CoreumMsg::AssetNFT(assetnft::Msg::Unfreeze {
        class_id: class_id.clone(),
        id: id.clone(),
    });

    Ok(Response::new()
        .add_attribute("method", "unfreeze")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

fn add_to_white_list(
    deps: DepsMut,
    info: MessageInfo,
    id: String,
    account: String,
) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;
    let cloned_account = account.clone();
    let msg = CoreumMsg::AssetNFT(assetnft::Msg::AddToWhitelist {
        class_id: class_id.clone(),
        id: id.clone(),
        account,
    });

    let yes = true;

    IS_WHITELISTED.save(deps.storage, &cloned_account, &yes)?;

    Ok(Response::new()
        .add_attribute("method", "add_to_white_list")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

fn remove_from_white_list(
    deps: DepsMut,
    info: MessageInfo,
    id: String,
    account: String,
) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;
    let cloned_account = account.clone();
    let msg = CoreumMsg::AssetNFT(assetnft::Msg::RemoveFromWhitelist {
        class_id: class_id.clone(),
        id: id.clone(),
        account,
    });
    let no = false;
    IS_WHITELISTED.save(deps.storage, &cloned_account, &no)?;
    Ok(Response::new()
        .add_attribute("method", "remove_from_white_list")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

fn send(
    deps: DepsMut,
    info: MessageInfo,
    id: String,
    receiver: String,
) -> CoreumResult<ContractError> {
    assert_owner(deps.storage, &info.sender)?;
    let class_id = CLASS_ID.load(deps.storage)?;

    let msg = CoreumMsg::NFT(nft::Msg::Send {
        class_id: class_id.clone(),
        id: id.clone(),
        receiver,
    });

    Ok(Response::new()
        .add_attribute("method", "send")
        .add_attribute("class_id", class_id)
        .add_attribute("id", id)
        .add_message(msg))
}

// ********** Queries **********

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<CoreumQueries>, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Params {} => to_binary(&query_params(deps)?),
        QueryMsg::Class {} => to_binary(&query_class(deps)?),
        QueryMsg::Classes { issuer } => to_binary(&query_classes(deps, issuer)?),
        QueryMsg::Frozen { id } => to_binary(&query_frozen(deps, id)?),
        QueryMsg::Whitelisted { id, account } => to_binary(&query_whitelisted(deps, id, account)?),
        QueryMsg::WhitelistedAccountsForNft { id } => {
            to_binary(&query_whitelisted_accounts_for_nft(deps, id)?)
        }
        QueryMsg::Balance { owner } => to_binary(&query_balance(deps, owner)?),
        QueryMsg::Owner { id } => to_binary(&query_owner(deps, id)?),
        QueryMsg::Supply {} => to_binary(&query_supply(deps)?),
        QueryMsg::Nft { id } => to_binary(&query_nft(deps, id)?),
        QueryMsg::Nfts { owner } => to_binary(&query_nfts(deps, owner)?),
        QueryMsg::ClassNft {} => to_binary(&query_nft_class(deps)?),
        QueryMsg::ClassesNft {} => to_binary(&query_nft_classes(deps)?),
        QueryMsg::BurntNft { nft_id } => to_binary(&query_burnt_nft(deps, nft_id)?),
        QueryMsg::BurntNftsInClass {} => to_binary(&query_burnt_nfts_in_class(deps)?),
        QueryMsg::GetInfo { owner } => to_binary(&get_info(deps, owner)?),
    }
}
fn get_info(deps: Deps<CoreumQueries>, owner: String) -> StdResult<GetInfoResponse> {
    let current_token_id = CURRENT_TOKEN_ID.load(deps.storage)?;
    let balance = query_balance(deps, owner.clone())?;
    let max_total_mint = MAX_TOTAL_MINT.load(deps.storage)?;

    let res = GetInfoResponse {
        current_token_id: current_token_id,
        balance: balance,
        max_total_mint: max_total_mint,
    };
    Ok(res)
}

fn query_params(deps: Deps<CoreumQueries>) -> StdResult<ParamsResponse> {
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::AssetNFT(assetnft::Query::Params {}).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_class(deps: Deps<CoreumQueries>) -> StdResult<ClassResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::AssetNFT(assetnft::Query::Class { id: class_id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_classes(deps: Deps<CoreumQueries>, issuer: String) -> StdResult<ClassesResponse> {
    let mut pagination = None;
    let mut classes = vec![];
    let mut res: ClassesResponse;
    loop {
        let request = CoreumQueries::AssetNFT(assetnft::Query::Classes {
            pagination,
            issuer: issuer.clone(),
        })
        .into();
        res = deps.querier.query(&request)?;
        classes.append(&mut res.classes);
        if res.pagination.next_key.is_none() {
            break;
        } else {
            pagination = Some(PageRequest {
                key: res.pagination.next_key,
                offset: None,
                limit: None,
                count_total: None,
                reverse: None,
            })
        }
    }
    let res = ClassesResponse {
        pagination: res.pagination,
        classes,
    };
    Ok(res)
}

fn query_frozen(deps: Deps<CoreumQueries>, id: String) -> StdResult<FrozenResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::AssetNFT(assetnft::Query::Frozen { id, class_id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_whitelisted(
    deps: Deps<CoreumQueries>,
    id: String,
    account: String,
) -> StdResult<WhitelistedResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::AssetNFT(assetnft::Query::Whitelisted {
            id,
            class_id,
            account,
        })
        .into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_whitelisted_accounts_for_nft(
    deps: Deps<CoreumQueries>,
    id: String,
) -> StdResult<WhitelistedAccountsForNFTResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let mut pagination = None;
    let mut accounts = vec![];
    let mut res: WhitelistedAccountsForNFTResponse;
    loop {
        let request = CoreumQueries::AssetNFT(assetnft::Query::WhitelistedAccountsForNFT {
            pagination,
            id: id.clone(),
            class_id: class_id.clone(),
        })
        .into();
        res = deps.querier.query(&request)?;
        accounts.append(&mut res.accounts);
        if res.pagination.next_key.is_none() {
            break;
        } else {
            pagination = Some(PageRequest {
                key: res.pagination.next_key,
                offset: None,
                limit: None,
                count_total: None,
                reverse: None,
            })
        }
    }
    let res = WhitelistedAccountsForNFTResponse {
        pagination: res.pagination,
        accounts,
    };
    Ok(res)
}

fn query_burnt_nft(deps: Deps<CoreumQueries>, nft_id: String) -> StdResult<BurntNFTResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::AssetNFT(assetnft::Query::BurntNFT { class_id, nft_id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_burnt_nfts_in_class(deps: Deps<CoreumQueries>) -> StdResult<BurntNFTsInClassResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let mut pagination = None;
    let mut nft_ids = vec![];
    let mut res: BurntNFTsInClassResponse;
    loop {
        let request = CoreumQueries::AssetNFT(assetnft::Query::BurntNFTsInClass {
            pagination,
            class_id: class_id.clone(),
        })
        .into();
        res = deps.querier.query(&request)?;
        nft_ids.append(&mut res.nft_ids);
        if res.pagination.next_key.is_none() {
            break;
        } else {
            pagination = Some(PageRequest {
                key: res.pagination.next_key,
                offset: None,
                limit: None,
                count_total: None,
                reverse: None,
            })
        }
    }
    let res = BurntNFTsInClassResponse {
        pagination: res.pagination,
        nft_ids,
    };
    Ok(res)
}

// ********** NFT **********

fn query_balance(deps: Deps<CoreumQueries>, owner: String) -> StdResult<nft::BalanceResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::NFT(nft::Query::Balance { class_id, owner }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_owner(deps: Deps<CoreumQueries>, id: String) -> StdResult<nft::OwnerResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::NFT(nft::Query::Owner { class_id, id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_supply(deps: Deps<CoreumQueries>) -> StdResult<nft::SupplyResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::NFT(nft::Query::Supply { class_id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_nft(deps: Deps<CoreumQueries>, id: String) -> StdResult<nft::NFTResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::NFT(nft::Query::NFT { class_id, id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_nfts(deps: Deps<CoreumQueries>, owner: Option<String>) -> StdResult<nft::NFTsResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let mut pagination = None;
    let mut nfts = vec![];
    let mut res: nft::NFTsResponse;
    if owner.is_none() {
        loop {
            let request = CoreumQueries::NFT(nft::Query::NFTs {
                class_id: Some(class_id.clone()),
                owner: None,
                pagination,
            })
            .into();
            res = deps.querier.query(&request)?;
            nfts.append(&mut res.nfts);
            if res.pagination.next_key.is_none() {
                break;
            } else {
                pagination = Some(PageRequest {
                    key: res.pagination.next_key,
                    offset: None,
                    limit: None,
                    count_total: None,
                    reverse: None,
                })
            }
        }
        let res = nft::NFTsResponse {
            nfts,
            pagination: res.pagination,
        };
        Ok(res)
    } else {
        loop {
            let request = CoreumQueries::NFT(nft::Query::NFTs {
                class_id: None,
                owner: Some(owner.clone().unwrap()),
                pagination,
            })
            .into();
            res = deps.querier.query(&request)?;
            nfts.append(&mut res.nfts);
            if res.pagination.next_key.is_none() {
                break;
            } else {
                pagination = Some(PageRequest {
                    key: res.pagination.next_key,
                    offset: None,
                    limit: None,
                    count_total: None,
                    reverse: None,
                })
            }
        }
        let res = nft::NFTsResponse {
            nfts,
            pagination: res.pagination,
        };
        Ok(res)
    }
}

fn query_nft_class(deps: Deps<CoreumQueries>) -> StdResult<nft::ClassResponse> {
    let class_id = CLASS_ID.load(deps.storage)?;
    let request: QueryRequest<CoreumQueries> =
        CoreumQueries::NFT(nft::Query::Class { class_id }).into();
    let res = deps.querier.query(&request)?;
    Ok(res)
}

fn query_nft_classes(deps: Deps<CoreumQueries>) -> StdResult<nft::ClassesResponse> {
    let mut pagination = None;
    let mut classes = vec![];
    let mut res: nft::ClassesResponse;
    loop {
        let request = CoreumQueries::NFT(nft::Query::Classes { pagination }).into();
        res = deps.querier.query(&request)?;
        classes.append(&mut res.classes);
        if res.pagination.next_key.is_none() {
            break;
        } else {
            pagination = Some(PageRequest {
                key: res.pagination.next_key,
                offset: None,
                limit: None,
                count_total: None,
                reverse: None,
            })
        }
    }
    let res = nft::ClassesResponse {
        classes,
        pagination: res.pagination,
    };
    Ok(res)
}
