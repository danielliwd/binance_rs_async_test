#[macro_use]
extern crate tracing;

use configurations;
use configurations::{Config as SysConfig, Opt};
use std::time::Duration;
use rust_decimal::prelude::*;
use tokio::time;

use binance::api::*;
use binance::account::*;
use binance::config::Config;
use binance::errors::Error as BinanceLibError;
use binance::general::*;
use binance::market::*;
use binance::rest_model::{OrderSide, OrderType, SymbolPrice, TimeInForce};

use binance::futures::general::*;
use binance::{api::Binance, futures::account::FuturesAccount};

use env_logger::Builder;
use dotenv::{dotenv, from_filename};
use std::process;

#[tokio::main]
async fn main() {

    dotenv().ok();
    
    Builder::new().parse_default_env().init();

    futures_task().await;

}

async fn futures_task(){

    let (conf, opt) = configurations::parse();


    let api_key = Some(conf.api_key.clone());
    let secret_key = Some(conf.api_sec.clone());

    let market: Market = Binance::new(None, None);
    // BINANCE_API_KEY BINANCE_API_SECRET_KEY
    let account: Account = Binance::new_with_env(&Config::default());


    info!("cancel all order for {}", conf.symbol);
    match account.cancel_all_open_orders(conf.symbol.clone()).await {
        Ok(answer) => info!("cancel order success! {:?}", answer),
        Err(e) => {
            error!("Error: {:?}", e);
        },
    };

    orderloop(&market, &account, &conf, &opt).await;

    info!("cancel all order for {}", conf.symbol);
    match account.cancel_all_open_orders(conf.symbol.clone()).await {
        Ok(answer) => info!("cancel order success! {:?}", answer),
        Err(e) => error!("Error: {:?}", e),
    };
}

async fn orderloop(market: &Market, account: &Account, conf: &SysConfig, _opt:&Opt){

    // interval
    let period = Duration::from_secs(conf.interval);

    let mut interval = time::interval(period);

    let mut order_id = None::<u64>;
    let mut open_counter = 0;
    let mut cancel_counter = 0;
    let max_order_n = conf.max_order_count; // 下单n次后退出
    let mut ask_avg_10: f64 = 0.0; // 10档均价
    let mut bid_avg_10: f64 = 0.0; // 10档均价
    loop{
        if cancel_counter >= max_order_n {
            break;
        }

        info!("open: {} cancel: {}", open_counter, cancel_counter);

        match order_id{
            Some(oid) => {
                if cancel_counter >= max_order_n {
                    break;
                }

                let order_cancellation = OrderCancellation {
                    symbol: conf.symbol.clone(),
                    order_id: Some(oid),
                    ..OrderCancellation::default()
                };

                match account.cancel_order(order_cancellation).await {
                    Ok(answer) => info!("{:?}", answer),
                    Err(e) => {
                        error!("Error: {e}");
                        process::exit(1);
                    },
                }

            },
            None => {
                if open_counter >= max_order_n {
                    continue;
                }
                match market.get_depth(&conf.symbol).await {
                    Ok(depth) => {
                        info!{"{:?}", depth};
                        ask_avg_10 = depth.asks.iter().take(10).map(|a|a.price).sum();
                        ask_avg_10 /= 10.0;
                        bid_avg_10 = depth.bids.iter().take(10).map(|a|a.price).sum();
                        bid_avg_10 /= 10.0;
                        info!("ask_avg:{} bid_avg:{}", ask_avg_10, bid_avg_10);
                    },
                    Err(e) => error!("get_depth: {}", e),
                }

                let sell_price = Decimal::from_f64(ask_avg_10 * 1.2).unwrap();
                let sell_amount = Decimal::from_f64(conf.order_size_usd as f64).unwrap() / sell_price;
                let price_decimal_place = 3;
                let size_decimal_place = 0;
                // TODO: add fn format_price_size_by_symbol(symbol, price, size)
                // TODO: add fn calc_size_for_symbol_price(symbol, price, size_usd)
                let sell_amount_f64 = sell_amount.round_dp(size_decimal_place).to_f64().unwrap();
                let sell_price_f64 = sell_price.round_dp(price_decimal_place).to_f64().unwrap();

                let limit_sell = OrderRequest {
                    symbol: conf.symbol.clone(),
                    quantity: Some(sell_amount_f64),
                    price: Some(sell_price_f64),
                    order_type: OrderType::Limit,
                    side: OrderSide::Sell,
                    time_in_force: Some(TimeInForce::FOK),
                    ..OrderRequest::default()
                };
                match account.place_order(limit_sell).await {
                    Ok(order) => {
                        info!("{:?}", order);
                        open_counter+=1;
                        order_id = Some(order.order_id);
                        break;
                    },
                    Err(e) => {
                        error!("limit sell Error: {:?}", e);
                        break;
                    },
                }
            },
        };

        interval.tick().await;
    };
}



