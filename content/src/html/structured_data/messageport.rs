//! The MessagePort-specific parts of the safe passing of structured data:
//! running its transfer steps and building its data holder (step 5.2 of
//! StructuredSerializeWithTransfer), and rebuilding the port on the
//! receiving side (StructuredDeserializeWithTransfer step 3.2).  The generic
//! algorithms live in [`super::safe_passing_of_structured_data`]; the
//! wire-format data holders (`PortTransferData`, `PortMessagePayload`) live
//! in `ipc_messages::safe_passing_of_structured_data` so a transfer can
//! cross processes.

use ipc_messages::safe_passing_of_structured_data::{PortTransferData, TransferDataHolder};

use crate::html::MessagePort;
use crate::html::structured_data::safe_passing_of_structured_data::Transferable;

use js_engine::{Completion, ExecutionContext};

type Types = crate::js::Types;

/// <https://html.spec.whatwg.org/#message-ports:transfer-steps>
/// (StructuredSerializeWithTransfer step 5.2's MessagePort branch).
pub(crate) fn transfer_steps(
    port: &MessagePort,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<TransferDataHolder, Types> {
    // StructuredSerializeWithTransfer step 5.2: "Otherwise, perform the
    // transfer steps for the interface identified by interfaceName."
    // Note: Only MessagePort is transferable here.  The data holder is built
    // here and the transfer steps run on the port
    // (`Transferable::transfer_steps`), which drain the port's record and
    // queue into the holder — the source object's [[Detached]] state is
    // implicit in the record having left this realm.
    let mut data_holder = PortTransferData {
        port_id: port.port_id,
        queue: Vec::new(),
        remote_port: None,
        in_flight: 0,
    };
    port.transfer_steps(&mut data_holder, ec)?;
    Ok(TransferDataHolder::MessagePort(data_holder))
}

/// <https://html.spec.whatwg.org/#message-ports:transfer-receiving-steps>
/// (StructuredDeserializeWithTransfer step 3.2's MessagePort branch).
pub(crate) fn transfer_receiving_steps(
    data_holder: &PortTransferData,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<MessagePort, Types> {
    // StructuredDeserializeWithTransfer step 3.2: "run the transfer-receiving
    // steps for MessagePort given dataHolder and a new MessagePort in
    // targetRealm."
    // Note: The new port is created in the current realm (the target realm)
    // first, then the transfer-receiving steps run on it
    // (`Transferable::transfer_receiving_steps`), which re-entangle it with
    // the remote port and register it with the user agent; the port's
    // platform object is the deserialized value.
    let port = MessagePort::new_port_with_id(data_holder.port_id, ec)?;
    port.transfer_receiving_steps(data_holder, ec)?;
    Ok(port)
}
