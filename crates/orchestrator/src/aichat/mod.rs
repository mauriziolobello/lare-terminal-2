//! # aichat — canale "AI Chat" (Slice 1a-net: core di rete)
//!
//! Gemello di `telegram/`: un modulo-canale che vive (in 1a-ui) come task del daemon.
//! Questa slice costruisce SOLO il core di rete: scoperta peer, elezione del server,
//! relay a stella, wire protocol peer↔peer e l'`AiChatChannel` integrativo.
//!
//! Principio architetturale: la rete trasporta SOLO testo + presenza. L'intelligenza
//! (l'AI) è locale a ogni macchina e arriva in 1b. Qui ci sono solo "etichette" umane.

pub mod peer;
pub mod election;
pub mod relay;
pub mod wire;
pub mod discovery;
pub mod transport;
pub mod room;
pub mod channel;
pub mod net;
pub mod config;
pub mod service;
