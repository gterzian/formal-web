//! Dedicated-worker domain code and the worker launcher.

pub(crate) mod dedicated_worker_agent;
pub(crate) mod dedicated_worker_global_scope;
pub(crate) mod worker;
pub(crate) mod worker_global_scope;
pub(crate) mod worker_location;
pub(crate) mod worker_navigator;

pub(crate) use dedicated_worker_global_scope::DedicatedWorkerGlobalScope;
pub(crate) use worker::Worker;
pub(crate) use worker::WorkerType;
pub(crate) use worker_global_scope::WorkerGlobalScope;
pub(crate) use worker_location::WorkerLocation;
pub(crate) use worker_navigator::WorkerNavigator;
