mod onboarding;

use log::{info, warn};
use std::time::Duration;

use clap::Parser;
use onboarding::{get_jwt_token, perform_onboarding, ParadexConfig};
use paradex::{
    rest::Client,
    structs::{ModifyOrderRequest, OrderRequest, OrderType, Side},
    url::URL,
};
use rust_decimal::{prelude::FromPrimitive, Decimal};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// 使用生产环境（默认为测试网）
    #[arg(long, action)]
    production: bool,
}

#[tokio::main]
async fn main() {
    // 初始化 rustls CryptoProvider（必须在任何网络操作之前）
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // 初始化日志
    simple_logger::init_with_level(log::Level::Info).unwrap();

    // 加载 .env 文件
    dotenvy::dotenv().ok();

    // 解析命令行参数
    let args = Args::parse();
    let url = if args.production {
        URL::Production
    } else {
        URL::Testnet
    };

    let symbol: String = "BTC-USD-PERP".into();
    let base_url = match url {
        URL::Production => "https://api.prod.paradex.trade/v1",
        URL::Testnet => "https://api.testnet.paradex.trade/v1",
    };

    // 从环境变量读取账户信息
    let private_key = std::env::var("paradex_account_private_key_hex").ok();
    let eth_account = std::env::var("eth_account_address").ok();
    let starknet_account = std::env::var("paradex_account_address").ok();

    // 根据是否提供私钥决定是否创建认证客户端
    let client_private = if let Some(private_key) = private_key {
        let config = if args.production {
            ParadexConfig::production()
        } else {
            ParadexConfig::testnet()
        };

        // 执行 onboarding（如果提供了以太坊账户和 StarkNet 账户）
        if let (Some(ref eth_addr), Some(ref starknet_addr)) = (&eth_account, &starknet_account) {
            info!("Performing onboarding...");
            let http_client = reqwest::Client::new();

            if let Err(e) = perform_onboarding(
                &http_client,
                base_url,
                starknet_addr,
                &private_key,
                eth_addr,
                &config,
            )
            .await
            {
                warn!("Onboarding failed (may already be onboarded): {}", e);
            } else {
                info!("Onboarding completed successfully");
            }

            // 获取 JWT token
            info!("Getting JWT token...");
            match get_jwt_token(&http_client, base_url, starknet_addr, &private_key, &config).await
            {
                Ok(jwt) => info!("JWT token obtained: {}...", &jwt[..jwt.len().min(20)]),
                Err(e) => warn!("Failed to get JWT token: {}", e),
            }
        } else {
            warn!("Ethereum or StarkNet account not provided. Skipping onboarding.");
        }

        // 创建 Paradex 客户端
        let client = Client::new(url, Some(private_key.clone())).await.unwrap();

        // 查询账户信息
        info!(
            "Account Information {:?}",
            client.account_information().await
        );
        info!("Balance {:?}", client.balance().await);
        info!("Positions {:?}", client.positions().await);

        Some((client, private_key))
    } else {
        None
    };

    // 创建 WebSocket 管理器
    // 如果有私钥，传入认证客户端；否则使用 None（仅公开数据）
    let manager = if let Some((ref client, _)) = client_private {
        paradex::ws::WebsocketManager::new(url, Some(client.clone())).await
    } else {
        paradex::ws::WebsocketManager::new(url, None).await
    };

    // 订阅公开市场数据频道
    let summary_id = manager
        .subscribe(
            paradex::ws::Channel::MarketSummary,
            Box::new(|message| info!("Received MarketSummary message {message:?}")),
        )
        .await
        .unwrap();

    let bbo_id = manager
        .subscribe(
            paradex::ws::Channel::BBO {
                market_symbol: symbol.clone(),
            },
            Box::new(|message| info!("Received BBO message {message:?}")),
        )
        .await
        .unwrap();

    let trades_id = manager
        .subscribe(
            paradex::ws::Channel::Trades {
                market_symbol: symbol.clone(),
            },
            Box::new(|message| info!("Received Trades message {message:?}")),
        )
        .await
        .unwrap();

    let orderbook_id = manager
        .subscribe(
            paradex::ws::Channel::OrderBook {
                channel_name: Some("orderbook".into()),
                market_symbol: symbol.clone(),
                refresh_rate: "50ms".into(),
                price_tick: None,
            },
            Box::new(|message| info!("Received OrderBook message {message:?}")),
        )
        .await
        .unwrap();

    let orderbook_deltas_id = manager
        .subscribe(
            paradex::ws::Channel::OrderBookDeltas {
                market_symbol: symbol.clone(),
            },
            Box::new(|message| info!("Received OrderBookDeltas message {message:?}")),
        )
        .await
        .unwrap();

    let funding_id = manager
        .subscribe(
            paradex::ws::Channel::FundingData {
                market_symbol: None,
            },
            Box::new(|message| info!("Received FundingData message {message:?}")),
        )
        .await
        .unwrap();

    // 订阅私有频道（仅在提供私钥时可用）
    let mut private_channel_ids = Vec::new();

    if client_private.is_some() {
        let orders_id = manager
            .subscribe(
                paradex::ws::Channel::Orders {
                    market_symbol: None,
                },
                Box::new(|message| info!("Received order update {message:?}")),
            )
            .await
            .unwrap();
        private_channel_ids.push(orders_id);

        let fills_id = manager
            .subscribe(
                paradex::ws::Channel::Fills {
                    market_symbol: None,
                },
                Box::new(|message| info!("Received fill {message:?}")),
            )
            .await
            .unwrap();
        private_channel_ids.push(fills_id);

        let position_id = manager
            .subscribe(
                paradex::ws::Channel::Position,
                Box::new(|message| info!("Received position {message:?}")),
            )
            .await
            .unwrap();
        private_channel_ids.push(position_id);

        let account_id = manager
            .subscribe(
                paradex::ws::Channel::Account,
                Box::new(|message| info!("Received account {message:?}")),
            )
            .await
            .unwrap();
        private_channel_ids.push(account_id);

        let balance_id = manager
            .subscribe(
                paradex::ws::Channel::BalanceEvents,
                Box::new(|message| info!("Received balance event {message:?}")),
            )
            .await
            .unwrap();
        private_channel_ids.push(balance_id);

        let funding_payments_id = manager
            .subscribe(
                paradex::ws::Channel::FundingPayments {
                    market_symbol: None,
                },
                Box::new(|message| info!("Received funding payment {message:?}")),
            )
            .await
            .unwrap();
        private_channel_ids.push(funding_payments_id);
    }

    // 等待 WebSocket 连接建立
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 如果有认证客户端，执行订单操作
    if let Some((ref client, _)) = client_private {
        // 创建订单
        let order_request = OrderRequest {
            instruction: paradex::structs::OrderInstruction::POST_ONLY,
            market: symbol.clone(),
            price: Decimal::from_f64(95000.0),
            side: Side::BUY,
            size: Decimal::from_f64(0.005).unwrap(),
            order_type: OrderType::LIMIT,
            client_id: Some("A".into()),
            flags: vec![],
            recv_window: None,
            stp: None,
            trigger_price: None,
        };

        info!("Sending order {order_request:?}");
        let result = client.create_order(order_request).await.unwrap();
        info!("Order result {result:?}");

        tokio::time::sleep(Duration::from_secs(5)).await;

        // 修改订单
        let modify_request = ModifyOrderRequest {
            id: result.id.clone(),
            market: symbol.clone(),
            price: Decimal::from_f64(92000.0),
            side: Side::BUY,
            size: Decimal::from_f64(0.005).unwrap(),
            order_type: OrderType::LIMIT,
        };

        info!("Sending modify order {modify_request:?}");
        let modify_result = client.modify_order(modify_request).await.unwrap();
        info!("Modify order result {modify_result:?}");

        tokio::time::sleep(Duration::from_secs(5)).await;

        // 取消订单
        info!(
            "Cancel Order Result {:?}",
            client.cancel_order(modify_result.id.clone()).await
        );

        info!(
            "Cancel by market orders Result {:?}",
            client.cancel_all_orders_for_market(symbol.clone()).await
        );

        info!(
            "Cancel All Orders Result {:?}",
            client.cancel_all_orders().await
        );
    }

    // 等待一段时间接收市场数据
    tokio::time::sleep(Duration::from_secs(120)).await;

    // 取消所有订阅
    let mut all_channel_ids = vec![
        summary_id,
        bbo_id,
        trades_id,
        orderbook_id,
        orderbook_deltas_id,
        funding_id,
    ];
    all_channel_ids.extend(private_channel_ids);

    for id in all_channel_ids {
        manager.unsubscribe(id).await.unwrap();
    }

    tokio::time::sleep(Duration::from_secs(5)).await;
    manager.stop().await.unwrap();
}
