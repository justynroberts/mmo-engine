//! The bag beyond the action bar, the bank, trading between players and the auction house.
//!
//! All of it rides the input action byte (`ACT_BAG`, `ACT_TRADE`, `ACT_AUCTION`, the operation in the high byte of
//! `arg`), and the answers ride the chat channel: the bank goes out with the bag in the INVENTORY packet (bag slots
//! first, then the bank's), a trade's state as a `channel::TRADE` line from the partner, and auction listings as
//! `channel::AUCTION` lines. A trade is checked and swapped in one step, so nothing is lost or doubled if a bag
//! changes while it is open. Listings and what is owed to sellers (`Post`) are kept with the realm's save, by
//! account tag and item name.

use serde::{Deserialize, Serialize};

use crate::items::Bag;
use crate::sim::{Sim, DRIVEN};
use proto::chat::channel;
use proto::game::{auction_op, bag_op, trade_op, BANK_SLOTS, INV_SLOTS};

/// Most stacks one side may put into a trade (the state line must fit a chat line).
pub const TRADE_STACKS: usize = 6;
/// Most listings in all, and the dearest price. How long one stays up, how many per seller, the trade and market
/// reaches are `GameFile::rules`.
pub const LISTINGS_MAX: usize = 5000;
pub const PRICE_MAX: u32 = 999_999;
/// Listings sent per page when browsing.
pub const PAGE: usize = 8;

/// A bank: kept like a bag, by slot.
pub type Vault = [(u8, u8); BANK_SLOTS];

/// One side of a trade: what is offered, as (item tag, count), the money, and whether they have accepted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    pub stacks: Vec<(u8, u32)>,
    pub coin: u32,
    pub ready: bool,
}

/// A trade between two players, by entity id (so it ends by itself when either leaves).
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub who: [u32; 2],
    pub side: [Side; 2],
}

/// Something for sale at the auction house.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Listing {
    pub id: u32,
    /// The seller's account tag, and the name shown.
    pub seller: String,
    pub seller_name: String,
    pub item: String,
    pub count: u8,
    pub price: u32,
    /// Unix seconds when it comes down unsold.
    pub ends: u64,
}

/// Owed to a player: money from a sale, or goods that did not sell. Handed over when they are about with room.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Post {
    pub owner: String,
    pub item: String,
    pub count: u32,
}

/// The market's live state.
#[derive(Default)]
pub struct Market {
    pub bank: Vec<Vault>,
    /// Per slot: the trade they are in (index + 1 into `trades`), and the player who asked them (entity id).
    pub asked_by: Vec<u32>,
    pub trades: Vec<Trade>,
    pub listings: Vec<Listing>,
    pub post: Vec<Post>,
    pub next_id: u32,
}

impl Market {
    pub fn new(cap: usize) -> Market {
        Market { bank: vec![[(0, 0); BANK_SLOTS]; cap], asked_by: vec![0; cap], next_id: 1, ..Default::default() }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Take `n` of item `tag` out of a bag (or vault). False, leaving it as it was, if there are not that many.
pub fn take(bag: &mut [(u8, u8)], tag: u8, n: u32) -> bool {
    let have: u32 = bag.iter().filter(|b| b.0 == tag).map(|b| b.1 as u32).sum();
    if tag == 0 || have < n {
        return false;
    }
    let mut left = n;
    for b in bag.iter_mut().rev() {
        if left == 0 {
            break;
        }
        if b.0 == tag {
            let t = (b.1 as u32).min(left);
            b.1 -= t as u8;
            left -= t;
            if b.1 == 0 {
                b.0 = 0;
            }
        }
    }
    true
}

/// Put `n` of item `tag` into a bag, topping up stacks first. Returns how many did not fit.
pub fn add(bag: &mut [(u8, u8)], tag: u8, n: u32, max: u8) -> u32 {
    let max = max.max(1) as u32;
    let mut left = n;
    for b in bag.iter_mut() {
        if left == 0 {
            break;
        }
        if b.0 == tag && (b.1 as u32) < max {
            let a = (max - b.1 as u32).min(left);
            b.1 += a as u8;
            left -= a;
        }
    }
    for b in bag.iter_mut() {
        if left == 0 {
            break;
        }
        if b.0 == 0 {
            let a = max.min(left);
            *b = (tag, a as u8);
            left -= a;
        }
    }
    left
}

impl Sim {
    fn bump_bag(&mut self, s: usize) {
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
    }

    fn stack_max(&self, tag: u8) -> u8 {
        self.items.get(tag as usize - 1).map_or(1, |d| d.stack_max)
    }

    fn live_player(&self, s: usize) -> bool {
        self.ents.alive[s] && self.ents.owner[s] < DRIVEN
    }

    /// Whether a player stands at a market: in a safe place (a town) or near a merchant.
    pub fn at_market(&self, s: usize) -> bool {
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        if self.places.iter().any(|p| p.safe && (p.x - x).powi(2) + (p.y - y).powi(2) <= p.r * p.r) {
            return true;
        }
        (0..self.ents.high).any(|t| {
            self.ents.alive[t]
                && self.archs.get(self.ents.arch[t] as usize).is_some_and(|a| !a.shop.is_empty())
                && (self.ents.x[t] - x).powi(2) + (self.ents.y[t] - y).powi(2) <= self.rules.market_reach * self.rules.market_reach
        })
    }

    /// The bag and the bank as the INVENTORY packet carries them: bag slots first, then the bank's.
    pub fn bag_and_bank(&self, s: usize) -> Vec<(u8, u8)> {
        let mut v = self.inv[s].to_vec();
        v.extend_from_slice(&self.market.bank[s]);
        v
    }

    pub fn reset_market(&mut self, s: usize) {
        self.cancel_trade(s, None);
        self.market.bank[s] = [(0, 0); BANK_SLOTS];
        self.market.asked_by[s] = 0;
    }

    // ------------------------------------------------------------------ bag and bank

    /// `ACT_BAG`: the operation in the high byte of `arg`, a slot in the low one, a second slot in `target`.
    pub fn bag_action(&mut self, s: usize, arg: u16, target: u32) -> bool {
        let (op, a, b) = ((arg >> 8) as u8, (arg & 0xff) as usize, target as usize);
        match op {
            bag_op::USE if a < INV_SLOTS => self.use_slot(s, a),
            bag_op::DROP if a < INV_SLOTS => self.drop_slot(s, a),
            bag_op::MOVE if a < INV_SLOTS && b < INV_SLOTS && a != b => {
                let mut bag = self.inv[s];
                let max = self.stack_max(bag[a].0.max(1));
                move_stack(&mut bag, a, b, max);
                self.inv[s] = bag;
                self.bump_bag(s);
                true
            }
            bag_op::SPLIT if a < INV_SLOTS => {
                let (tag, n) = self.inv[s][a];
                let Some(e) = self.inv[s].iter().position(|x| x.0 == 0) else { return false };
                if tag == 0 || n < 2 {
                    return false;
                }
                self.inv[s][a].1 = n - n / 2;
                self.inv[s][e] = (tag, n / 2);
                self.bump_bag(s);
                true
            }
            bag_op::DEPOSIT | bag_op::WITHDRAW | bag_op::BANK_MOVE => self.bank_action(s, op, a, b),
            _ => false,
        }
    }

    fn bank_action(&mut self, s: usize, op: u8, a: usize, b: usize) -> bool {
        if self.trade_of(s).is_some() {
            return false;
        }
        if !self.at_market(s) {
            self.tells.push((s, "Your bank is kept in the towns: go to one, or to a merchant.".into()));
            return false;
        }
        match op {
            bag_op::DEPOSIT if a < INV_SLOTS => {
                let (tag, n) = self.inv[s][a];
                if tag == 0 {
                    return false;
                }
                let max = self.stack_max(tag);
                let left = add(&mut self.market.bank[s], tag, n as u32, max);
                if left == n as u32 {
                    self.tells.push((s, "Your bank is full.".into()));
                    return false;
                }
                self.inv[s][a] = if left == 0 { (0, 0) } else { (tag, left as u8) };
            }
            bag_op::WITHDRAW if a < BANK_SLOTS => {
                let (tag, n) = self.market.bank[s][a];
                if tag == 0 {
                    return false;
                }
                let max = self.stack_max(tag);
                let left = add(&mut self.inv[s], tag, n as u32, max);
                if left == n as u32 {
                    self.tells.push((s, "Your bag is full.".into()));
                    return false;
                }
                self.market.bank[s][a] = if left == 0 { (0, 0) } else { (tag, left as u8) };
            }
            bag_op::BANK_MOVE if a < BANK_SLOTS && b < BANK_SLOTS && a != b => {
                let max = self.stack_max(self.market.bank[s][a].0.max(1));
                move_stack(&mut self.market.bank[s], a, b, max);
            }
            _ => return false,
        }
        self.bump_bag(s);
        true
    }

    /// The bank as names, for saving.
    pub fn bank_names(&self, s: usize) -> Vec<(String, u8)> {
        self.market.bank[s].iter().filter(|b| b.0 != 0).filter_map(|&(t, c)| Some((self.items.get(t as usize - 1)?.name.clone(), c))).collect()
    }

    pub fn restore_bank(&mut self, s: usize, saved: &[(String, u8)]) {
        self.market.bank[s] = [(0, 0); BANK_SLOTS];
        for (name, c) in saved {
            if let Some(i) = self.items.iter().position(|x| &x.name == name) {
                let max = self.items[i].stack_max;
                add(&mut self.market.bank[s], i as u8 + 1, *c as u32, max);
            }
        }
        self.bump_bag(s);
    }

    // ------------------------------------------------------------------ trading

    fn trade_of(&self, s: usize) -> Option<usize> {
        let me = self.ents.id_of(s);
        self.market.trades.iter().position(|t| t.who.contains(&me))
    }

    /// `ACT_TRADE`.
    pub fn trade_action(&mut self, s: usize, arg: u16, target: u32) -> bool {
        let (op, a) = ((arg >> 8) as u8, (arg & 0xff) as usize);
        if op == trade_op::ASK {
            return self.ask_trade(s, target);
        }
        if op == trade_op::CANCEL {
            if self.trade_of(s).is_none() && self.market.asked_by[s] == 0 {
                return false;
            }
            self.market.asked_by[s] = 0;
            self.cancel_trade(s, Some("The trade is off."));
            return true;
        }
        let Some(k) = self.trade_of(s) else { return false };
        let i = (self.market.trades[k].who[1] == self.ents.id_of(s)) as usize;
        match op {
            trade_op::OFFER if a < INV_SLOTS => {
                let (tag, have) = self.inv[s][a];
                if tag == 0 || Some(tag as usize - 1) == self.currency {
                    return false;
                }
                let want = if target == 0 { have as u32 } else { target.min(have as u32) };
                let side = &mut self.market.trades[k].side[i];
                // Never more than is carried of it, however many times it is offered.
                let total: u32 = self.inv[s].iter().filter(|b| b.0 == tag).map(|b| b.1 as u32).sum();
                if let Some(x) = side.stacks.iter_mut().find(|x| x.0 == tag) {
                    x.1 = (x.1 + want).min(total);
                } else if side.stacks.len() < TRADE_STACKS {
                    side.stacks.push((tag, want));
                } else {
                    return false;
                }
            }
            trade_op::TAKE_BACK => {
                let side = &mut self.market.trades[k].side[i];
                if a >= side.stacks.len() {
                    return false;
                }
                side.stacks.remove(a);
            }
            trade_op::COIN => {
                if self.currency.is_none() {
                    return false;
                }
                self.market.trades[k].side[i].coin = target.min(self.money(s));
            }
            trade_op::READY => {
                self.market.trades[k].side[i].ready = true;
                if self.market.trades[k].side.iter().all(|x| x.ready) {
                    return self.finish_trade(k);
                }
                self.send_trade(k);
                return true;
            }
            _ => return false,
        }
        // Any change to what is on the table means both look again.
        for x in self.market.trades[k].side.iter_mut() {
            x.ready = false;
        }
        self.send_trade(k);
        true
    }

    fn ask_trade(&mut self, s: usize, target: u32) -> bool {
        let Some(t) = self.ents.resolve(target).filter(|&t| t != s && self.live_player(t)) else { return false };
        if !self.live_player(s) {
            return false;
        }
        let d2 = (self.ents.x[t] - self.ents.x[s]).powi(2) + (self.ents.y[t] - self.ents.y[s]).powi(2);
        if d2 > self.rules.trade_reach * self.rules.trade_reach {
            self.tells.push((s, "Too far away to trade.".into()));
            return false;
        }
        if self.trade_of(s).is_some() || self.trade_of(t).is_some() {
            self.tells.push((s, "One of you is already trading.".into()));
            return false;
        }
        let (me, them) = (self.ents.id_of(s), self.ents.id_of(t));
        if self.market.asked_by[s] == them {
            // They asked first: this is the yes.
            self.market.asked_by[s] = 0;
            self.market.trades.push(Trade { who: [them, me], side: Default::default() });
            let k = self.market.trades.len() - 1;
            self.send_trade(k);
            return true;
        }
        self.market.asked_by[t] = me;
        self.notes.push((t, me, channel::TRADE, "?".into()));
        let name = self.name_of(t);
        self.tells.push((s, format!("You ask {name} to trade.")));
        true
    }

    /// Call off the trade `s` is in, telling both why (if `why`).
    pub fn cancel_trade(&mut self, s: usize, why: Option<&str>) {
        let Some(k) = self.trade_of(s) else { return };
        let t = self.market.trades.remove(k);
        for id in t.who {
            if let Some(p) = self.ents.resolve(id) {
                self.notes.push((p, 0, channel::TRADE, "-".into()));
                if let Some(w) = why {
                    self.tells.push((p, w.into()));
                }
            }
        }
    }

    /// Tell both sides what is on the table: "mine|coin|ready|theirs|coin|ready", stacks as "tag:count,...".
    fn send_trade(&mut self, k: usize) {
        let t = self.market.trades[k].clone();
        let line = |x: &Side| format!("{}|{}|{}", x.stacks.iter().map(|(g, n)| format!("{g}:{n}")).collect::<Vec<_>>().join(","), x.coin, x.ready as u8);
        for i in 0..2 {
            if let Some(p) = self.ents.resolve(t.who[i]) {
                self.notes.push((p, t.who[1 - i], channel::TRADE, format!("{}|{}", line(&t.side[i]), line(&t.side[1 - i]))));
            }
        }
    }

    /// Both have accepted: swap, all or nothing.
    fn finish_trade(&mut self, k: usize) -> bool {
        let t = self.market.trades[k].clone();
        let (Some(a), Some(b)) = (self.ents.resolve(t.who[0]), self.ents.resolve(t.who[1])) else {
            self.market.trades.remove(k);
            return false;
        };
        let coin = self.currency.map(|c| c as u8 + 1);
        let gives = |x: &Side| -> Vec<(u8, u32)> {
            let mut v = x.stacks.clone();
            if let (Some(c), true) = (coin, x.coin > 0) {
                v.push((c, x.coin));
            }
            v
        };
        let (ga, gb) = (gives(&t.side[0]), gives(&t.side[1]));
        let (mut ba, mut bb): (Bag, Bag) = (self.inv[a], self.inv[b]);
        let mut ok = ga.iter().all(|&(g, n)| take(&mut ba, g, n)) && gb.iter().all(|&(g, n)| take(&mut bb, g, n));
        if ok {
            ok = gb.iter().all(|&(g, n)| add(&mut ba, g, n, self.stack_max(g)) == 0) && ga.iter().all(|&(g, n)| add(&mut bb, g, n, self.stack_max(g)) == 0);
            if !ok {
                for p in [a, b] {
                    self.tells.push((p, "There is not room in a bag for that trade.".into()));
                }
            }
        } else {
            for p in [a, b] {
                self.tells.push((p, "Something offered is no longer there.".into()));
            }
        }
        if !ok {
            for x in self.market.trades[k].side.iter_mut() {
                x.ready = false;
            }
            self.send_trade(k);
            return false;
        }
        self.inv[a] = ba;
        self.inv[b] = bb;
        self.bump_bag(a);
        self.bump_bag(b);
        self.market.trades.remove(k);
        for (p, q) in [(a, b), (b, a)] {
            self.notes.push((p, 0, channel::TRADE, "-".into()));
            let name = self.name_of(q);
            self.tells.push((p, format!("Traded with {name}.")));
        }
        true
    }

    /// Once a second: trades whose players left or walked apart are called off.
    pub fn system_market(&mut self) {
        let mut k = 0;
        while k < self.market.trades.len() {
            let t = &self.market.trades[k];
            let (a, b) = (self.ents.resolve(t.who[0]), self.ents.resolve(t.who[1]));
            let apart = match (a, b) {
                (Some(a), Some(b)) => {
                    !self.live_player(a) || !self.live_player(b) || (self.ents.x[a] - self.ents.x[b]).powi(2) + (self.ents.y[a] - self.ents.y[b]).powi(2) > self.rules.trade_break * self.rules.trade_break
                }
                _ => true,
            };
            if apart {
                let t = self.market.trades.remove(k);
                for id in t.who {
                    if let Some(p) = self.ents.resolve(id) {
                        self.notes.push((p, 0, channel::TRADE, "-".into()));
                        self.tells.push((p, "The trade is off: you are too far apart.".into()));
                    }
                }
            } else {
                k += 1;
            }
        }
        self.system_auction();
    }

    // ------------------------------------------------------------------ the auction house

    /// `ACT_AUCTION`.
    pub fn auction_action(&mut self, s: usize, arg: u16, target: u32) -> bool {
        let (op, a) = ((arg >> 8) as u8, (arg & 0xff) as usize);
        let Some(me) = self.tag_of(s) else {
            self.tells.push((s, "Guests cannot use the auction house.".into()));
            return false;
        };
        if !self.at_market(s) {
            self.tells.push((s, "The auction house is in the towns: go to one, or to a merchant.".into()));
            return false;
        }
        match op {
            auction_op::BROWSE => {
                self.send_listings(s, &me, a, target as u8, false);
                true
            }
            auction_op::MINE => {
                self.send_listings(s, &me, a, 0, true);
                true
            }
            auction_op::SELL if a < INV_SLOTS => self.list_item(s, &me, a, target),
            auction_op::BUY => self.buy_listing(s, &me, target),
            auction_op::CANCEL => {
                let Some(k) = self.market.listings.iter().position(|l| l.id == target && l.seller == me) else { return false };
                let l = self.market.listings.remove(k);
                self.market.post.push(Post { owner: me.clone(), item: l.item, count: l.count as u32 });
                self.rs.dirty = true;
                self.send_listings(s, &me, 0, 0, true);
                true
            }
            _ => false,
        }
    }

    fn list_item(&mut self, s: usize, me: &str, a: usize, price: u32) -> bool {
        if self.currency.is_none() {
            return false;
        }
        let (tag, n) = self.inv[s][a];
        if tag == 0 || Some(tag as usize - 1) == self.currency || self.trade_of(s).is_some() {
            return false;
        }
        if price == 0 || price > PRICE_MAX {
            self.tells.push((s, format!("A price must be 1 to {PRICE_MAX}.")));
            return false;
        }
        if self.market.listings.iter().filter(|l| l.seller == me).count() >= self.rules.listings_per_seller as usize || self.market.listings.len() >= LISTINGS_MAX {
            self.tells.push((s, format!("You have as many things up for sale as you may ({}).", self.rules.listings_per_seller)));
            return false;
        }
        let item = self.items[tag as usize - 1].name.clone();
        self.inv[s][a] = (0, 0);
        self.bump_bag(s);
        let id = self.market.next_id;
        self.market.next_id = self.market.next_id.wrapping_add(1).max(1);
        let seller_name = self.name_of(s).to_string();
        self.market.listings.push(Listing { id, seller: me.to_string(), seller_name, item: item.clone(), count: n, price, ends: unix_now() + (self.rules.listing_hours * 3600.0) as u64 });
        self.rs.dirty = true;
        self.tells.push((s, format!("{n} {item} put up for {price}.")));
        self.send_listings(s, me, 0, 0, true);
        true
    }

    fn buy_listing(&mut self, s: usize, me: &str, id: u32) -> bool {
        let Some(c) = self.currency else { return false };
        let Some(k) = self.market.listings.iter().position(|l| l.id == id) else {
            self.tells.push((s, "That has gone.".into()));
            return false;
        };
        let l = self.market.listings[k].clone();
        if l.seller == me {
            return false;
        }
        let Some(i) = self.items.iter().position(|x| x.name == l.item) else { return false };
        if self.money(s) < l.price {
            self.tells.push((s, "You cannot afford that.".into()));
            return false;
        }
        let mut bag = self.inv[s];
        if !take(&mut bag, c as u8 + 1, l.price) || add(&mut bag, i as u8 + 1, l.count as u32, self.items[i].stack_max) > 0 {
            self.tells.push((s, "Your bag is full.".into()));
            return false;
        }
        self.inv[s] = bag;
        self.bump_bag(s);
        self.market.listings.remove(k);
        let money = self.items[c].name.clone();
        self.market.post.push(Post { owner: l.seller.clone(), item: money, count: l.price });
        self.rs.dirty = true;
        self.tells.push((s, format!("Bought {} {} for {}.", l.count, l.item, l.price)));
        for p in 0..self.ents.high {
            if self.live_player(p) && self.tag_of(p).as_deref() == Some(l.seller.as_str()) {
                self.tells.push((p, format!("Your {} {} sold for {}.", l.count, l.item, l.price)));
            }
        }
        true
    }

    /// Send a page of listings as AUCTION lines: "H page pages total", then "R id tag count price secs mine seller"
    /// for each, then "E". `item` (a tag, 0 for any) narrows it; `mine` shows only the player's own.
    fn send_listings(&mut self, s: usize, me: &str, page: usize, item: u8, mine: bool) {
        let name = (item != 0).then(|| self.items.get(item as usize - 1).map(|d| d.name.clone())).flatten();
        let mut hits: Vec<&Listing> = self.market.listings.iter().filter(|l| if mine { l.seller == me } else { name.as_ref().is_none_or(|n| &l.item == n) }).collect();
        hits.sort_by(|a, b| a.item.cmp(&b.item).then(a.price.cmp(&b.price)).then(a.id.cmp(&b.id)));
        let pages = hits.len().div_ceil(PAGE).max(1);
        let page = page.min(pages - 1);
        let now = unix_now();
        let mut lines = vec![format!("H {page} {pages} {} {}", hits.len(), mine as u8)];
        for l in hits.iter().skip(page * PAGE).take(PAGE) {
            let tag = self.items.iter().position(|x| x.name == l.item).map_or(0, |i| i + 1);
            let seller: String = l.seller_name.chars().take(24).collect();
            lines.push(format!("R {} {tag} {} {} {} {} {seller}", l.id, l.count, l.price, l.ends.saturating_sub(now), (l.seller == me) as u8));
        }
        lines.push("E".into());
        for line in lines {
            self.notes.push((s, 0, channel::AUCTION, line));
        }
    }

    /// Listings that ran out go back to their sellers, and what is owed is handed to those about with room.
    fn system_auction(&mut self) {
        let now = unix_now();
        if self.market.listings.iter().any(|l| l.ends <= now) {
            let (gone, kept): (Vec<Listing>, Vec<Listing>) = std::mem::take(&mut self.market.listings).into_iter().partition(|l| l.ends <= now);
            self.market.listings = kept;
            for l in gone {
                self.market.post.push(Post { owner: l.seller, item: l.item, count: l.count as u32 });
            }
            self.rs.dirty = true;
        }
        if self.market.post.is_empty() || self.tick % (5 * self.tick_hz.max(1)) != 0 {
            return;
        }
        for s in 0..self.ents.high {
            if !self.live_player(s) {
                continue;
            }
            let Some(me) = self.tag_of(s) else { continue };
            let mut k = 0;
            while k < self.market.post.len() {
                if self.market.post[k].owner != me {
                    k += 1;
                    continue;
                }
                let Some(i) = self.items.iter().position(|x| x.name == self.market.post[k].item) else {
                    self.market.post.remove(k); // an item the game no longer has
                    continue;
                };
                let n = self.market.post[k].count;
                let left = self.give(s, i, n);
                if left < n {
                    self.tells.push((s, format!("A runner from the auction house brings you {} {}.", n - left, self.items[i].shown())));
                    self.rs.dirty = true;
                }
                if left == 0 {
                    self.market.post.remove(k);
                } else {
                    self.market.post[k].count = left;
                    break; // the bag is full
                }
            }
        }
    }
}

/// Move the stack in `a` onto `b`: topping it up if it is the same item, swapping places otherwise.
fn move_stack(bag: &mut [(u8, u8)], a: usize, b: usize, max: u8) {
    let (x, y) = (bag[a], bag[b]);
    if x.0 != 0 && x.0 == y.0 {
        let room = max.saturating_sub(y.1).min(x.1);
        bag[b].1 += room;
        bag[a].1 -= room;
        if bag[a].1 == 0 {
            bag[a] = (0, 0);
        }
    } else {
        bag.swap(a, b);
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim};
    use proto::game::{auction_op, bag_op, trade_op, GameFile, ItemDef, ACT_AUCTION, ACT_BAG, ACT_TRADE, INV_SLOTS};
    use proto::world::World;

    fn item(name: &str, stack: u8) -> ItemDef {
        ItemDef { name: name.into(), color: "#ffffff".into(), stack_max: stack, uses: vec![], consume: true, value: 1, equip: None, mount: 0.0, rarity: Default::default(), title: String::new(), block: None }
    }

    fn town() -> proto::game::Place {
        serde_json::from_str(r#"{"name":"town","x":100,"y":100,"r":50,"safe":true}"#).unwrap()
    }

    /// Two players side by side, and the indexes of silver (the money), bread and a sword.
    fn world() -> (Sim, usize, usize, usize, usize, usize) {
        let mut g = GameFile::default_game();
        g.items.extend([item("silver", 99), item("bread", 20), item("sword", 1)]);
        g.currency = Some("silver".into());
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let at = |s: &Sim, n: &str| s.items.iter().position(|i| i.name == n).unwrap();
        let (silver, bread, sword) = (at(&s, "silver"), at(&s, "bread"), at(&s, "sword"));
        let [a, b] = [0, 1].map(|o| s.spawn(o).unwrap());
        for (p, x, tag) in [(a, 100.0, "acc-a"), (b, 104.0, "acc-b")] {
            s.ents.x[p] = x;
            s.ents.y[p] = 100.0;
            s.set_account(p, Some(tag));
            s.set_name(p, if p == a { "Aoife" } else { "Bran" }, true);
        }
        (s, a, b, silver, bread, sword)
    }

    fn act(s: &mut Sim, slot: usize, action: u8, target: u32, arg: u16) {
        s.item_cd[slot] = 0;
        s.step(&[PendingAction { slot, action, target, arg }]);
    }

    fn count(s: &Sim, p: usize, tag: u8) -> u32 {
        s.inv[p].iter().filter(|b| b.0 == tag).map(|b| b.1 as u32).sum()
    }

    #[test]
    fn the_bag_moves_splits_and_banks() {
        let (mut s, a, _, _, bread, _) = world();
        assert_eq!(s.inv[a].len(), INV_SLOTS);
        s.give(a, bread, 10);
        act(&mut s, a, ACT_BAG, 20, (bag_op::MOVE as u16) << 8);
        assert_eq!((s.inv[a][0], s.inv[a][20]), ((0, 0), (bread as u8 + 1, 10)));
        act(&mut s, a, ACT_BAG, 0, (bag_op::SPLIT as u16) << 8 | 20);
        assert_eq!((s.inv[a][0], s.inv[a][20]), ((bread as u8 + 1, 5), (bread as u8 + 1, 5)));
        // The bank is only in town.
        act(&mut s, a, ACT_BAG, 0, (bag_op::DEPOSIT as u16) << 8 | 20);
        assert_eq!(s.market.bank[a][0], (0, 0));
        s.places.push(town());
        act(&mut s, a, ACT_BAG, 0, (bag_op::DEPOSIT as u16) << 8 | 20);
        assert_eq!(s.market.bank[a][0], (bread as u8 + 1, 5));
        assert_eq!(s.bag_and_bank(a).len(), INV_SLOTS + proto::game::BANK_SLOTS);
        let saved = s.bank_names(a);
        act(&mut s, a, ACT_BAG, 0, (bag_op::WITHDRAW as u16) << 8);
        assert_eq!(count(&s, a, bread as u8 + 1), 10);
        s.restore_bank(a, &saved);
        assert_eq!(s.market.bank[a][0], (bread as u8 + 1, 5));
    }

    #[test]
    fn a_trade_swaps_both_sides_at_once_or_not_at_all() {
        let (mut s, a, b, silver, bread, sword) = world();
        let coin = silver as u8 + 1;
        s.give(a, bread, 6);
        s.give(b, silver, 30);
        s.give(b, sword, 1);
        let (ia, ib) = (s.ents.id_of(a), s.ents.id_of(b));
        act(&mut s, a, ACT_TRADE, ib, 0);
        assert!(s.notes.iter().any(|n| n.0 == b && n.3 == "?"));
        act(&mut s, b, ACT_TRADE, ia, 0);
        assert_eq!(s.market.trades.len(), 1);
        act(&mut s, a, ACT_TRADE, 4, (trade_op::OFFER as u16) << 8);
        let sw = s.inv[b].iter().position(|x| x.0 == sword as u8 + 1).unwrap() as u16;
        act(&mut s, b, ACT_TRADE, 0, (trade_op::OFFER as u16) << 8 | sw);
        act(&mut s, b, ACT_TRADE, 20, (trade_op::COIN as u16) << 8);
        act(&mut s, a, ACT_TRADE, 0, (trade_op::READY as u16) << 8);
        // b changes the offer: a's yes no longer stands.
        act(&mut s, b, ACT_TRADE, 12, (trade_op::COIN as u16) << 8);
        assert!(!s.market.trades[0].side[0].ready);
        act(&mut s, a, ACT_TRADE, 0, (trade_op::READY as u16) << 8);
        // a eats the bread before b accepts: the trade cannot go through.
        s.inv[a][0] = (0, 0);
        act(&mut s, b, ACT_TRADE, 0, (trade_op::READY as u16) << 8);
        assert_eq!(s.market.trades.len(), 1);
        assert_eq!(count(&s, b, sword as u8 + 1), 1);
        s.give(a, bread, 6);
        act(&mut s, a, ACT_TRADE, 0, (trade_op::READY as u16) << 8);
        act(&mut s, b, ACT_TRADE, 0, (trade_op::READY as u16) << 8);
        assert!(s.market.trades.is_empty());
        assert_eq!((count(&s, a, bread as u8 + 1), count(&s, a, sword as u8 + 1), count(&s, a, coin)), (2, 1, 12));
        assert_eq!((count(&s, b, bread as u8 + 1), count(&s, b, sword as u8 + 1), count(&s, b, coin)), (4, 0, 18));
        // Walking apart calls a trade off.
        act(&mut s, a, ACT_TRADE, ib, 0);
        act(&mut s, b, ACT_TRADE, ia, 0);
        s.ents.x[b] = 200.0;
        for _ in 0..20 {
            s.step(&[]);
        }
        assert!(s.market.trades.is_empty());
    }

    #[test]
    fn the_auction_house_sells_pays_the_seller_and_returns_what_is_left() {
        let (mut s, a, b, silver, bread, sword) = world();
        let coin = silver as u8 + 1;
        s.places.push(town());
        s.give(a, sword, 1);
        s.give(a, bread, 3);
        s.give(b, silver, 40);
        act(&mut s, a, ACT_AUCTION, 25, (auction_op::SELL as u16) << 8);
        act(&mut s, a, ACT_AUCTION, 9, (auction_op::SELL as u16) << 8 | 1);
        assert_eq!(s.market.listings.len(), 2);
        assert_eq!(count(&s, a, sword as u8 + 1), 0);
        s.notes.clear();
        act(&mut s, b, ACT_AUCTION, 0, (auction_op::BROWSE as u16) << 8);
        let lines: Vec<&str> = s.notes.iter().filter(|n| n.0 == b).map(|n| n.3.as_str()).collect();
        assert_eq!(lines[0], "H 0 1 2 0");
        assert!(lines[1].starts_with("R ") && lines[1].ends_with(" 0 Aoife"), "{lines:?}");
        let id = s.market.listings.iter().find(|l| l.item == "sword").unwrap().id;
        act(&mut s, b, ACT_AUCTION, id, (auction_op::BUY as u16) << 8);
        assert_eq!((count(&s, b, sword as u8 + 1), count(&s, b, coin)), (1, 15));
        // The seller is paid within a few seconds; the bread runs out and comes home.
        s.market.listings[0].ends = 0;
        for _ in 0..120 {
            s.step(&[]);
        }
        assert_eq!((count(&s, a, coin), count(&s, a, bread as u8 + 1)), (25, 3));
        assert!(s.market.post.is_empty() && s.market.listings.is_empty());
        // And it all survives a restart.
        let at = s.inv[a].iter().position(|x| x.0 == bread as u8 + 1).unwrap() as u16;
        act(&mut s, a, ACT_AUCTION, 7, (auction_op::SELL as u16) << 8 | at);
        let saved = s.realm_saved();
        let text = serde_json::to_string(&saved).unwrap();
        let (mut t, ..) = world();
        t.realm_restore(serde_json::from_str(&text).unwrap());
        assert_eq!(t.market.listings.len(), 1);
    }
}
