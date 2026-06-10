# content/src/js

`content/src/js` integrates Boa (the JavaScript engine) with the content
process and keeps JavaScript-facing wrapper identity separate from DOM and
HTML [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object)
state.

- `content/src/html/environment_settings_object.rs` owns the Boa `Context`,
  global-object construction, and the Rust state that corresponds to an HTML
  environment settings object.
- `content/src/html/global_scope.rs` owns per-global wrapper caches and
  callback state so repeated lookups reuse the same `JsObject` identity.
- `html_parser.rs` bridges html5ever parsing to Blitz mutations, records
  parser errors, and collects parser-discovered classic scripts.
- **`content/src/js/bindings/` is the single home for Web IDL binding
  definitions** — DOM, HTML, Streams, WebAssembly, CSS, or any other spec.
  Each binding:
  - Implements `WebIdlInterface` or `WebIdlNamespace` to define *which
    members* the interface or namespace exposes.
  - Provides thin getter/setter/method functions that convert JavaScript
    arguments and delegate to domain-level implementations.
  - Uses the Web IDL bindings infrastructure (`WebIdlInterface`,
    `WebIdlNamespace`, `register_interface_spec`, `register_namespace_spec`,
    etc.) from `content/src/webidl/bindings/` instead of calling Boa directly.
- **Spec algorithms and interface implementations do NOT go in
  bindings.**  Spec-mapped code belongs in the owning domain directory
  (`content/src/dom/`, `content/src/html/`, `content/src/streams/`,
  `content/src/wasm/`).  The bindings layer calls into domain functions;
  it does not reimplement them.
- The binding code should convert arguments, check [inherited
  interfaces](https://webidl.spec.whatwg.org/#dfn-inherited-interfaces) to
  identify the platform object's type, and delegate to the platform object
  or domain function.
- Run microtask checkpoints at task boundaries rather than after every
  Rust-to-JavaScript callback.
- Document process structs against HTML concepts such as
  `#environment-settings-object` and `#global-object`, not as ad hoc DOM
  interfaces.

## Exotic objects

Some HTML spec objects (WindowProxy, Location) require exotic internal methods.
Boa supports custom internal methods via `JsData::internal_methods()` returning
a custom `InternalObjectMethods` vtable. See `content/src/webidl/README.md` for
the exotic-object implementation pattern.

### Working with vendored boa: use spec links, not visibility changes

When implementing a spec algorithm that references ECMAScript operations,
**never modify `vendor/boa/`** to make internal APIs public. Instead, follow
this methodology:

1. Read the relevant spec (e.g. HTML §7.2.3 The WindowProxy exotic object)
   using `spec_lookup`.
2. Look at the **index of links** at the bottom of the spec section — each
   JS operation references an ECMAScript spec algorithm by URL, e.g.
   [`OrdinaryGetPrototypeOf`](https://tc39.es/ecma262/#sec-ordinarygetprototypeof)
   or [`OrdinaryGetOwnProperty`](https://tc39.es/ecma262/#sec-ordinarygetownproperty).
3. Search `vendor/boa/` for that exact ECMAScript spec link using `grep`:
   ```bash
   grep -rn "tc39.es/ecma262/#sec-xxx" vendor/boa/
   ```
   Boa code is documented with spec links, so this finds the exact function
   implementing that operation.
4. Check if the function is **already public**. If not, look for a
   **higher-level public wrapper**:

   | ECMAScript operation | `pub(crate)` impl in boa | Already-public equivalent |
   |---|---|---|
   | `OrdinaryGetPrototypeOf` | `ordinary_get_prototype_of` | `JsObject::prototype()` |
   | `OrdinaryIsExtensible` | `ordinary_is_extensible` | `JsObject::is_extensible(context)` |
   | `OrdinaryGet` | `ordinary_get` | `JsObject::get(key, context)` |
   | `OrdinarySet` | `ordinary_set` | `JsObject::set(key, value, throw, context)` |
   | `OrdinaryDelete` | `ordinary_delete` | `JsObject::delete_property_or_throw(key, context)` |
   | `OrdinaryHasProperty` | `ordinary_has_property` | `JsObject::has_property(key, context)` |
   | `OrdinaryOwnPropertyKeys` | `ordinary_own_property_keys` | `JsObject::own_property_keys(context)` |

5. When no existing public method covers the exact operation needed (e.g.,
   getting a raw `PropertyDescriptor` for [[GetOwnProperty]], or passing a
   custom receiver for [[Set]]/[[Get]]), add a **new public wrapper method**
   to `JsObject` in `vendor/boa/core/engine/src/object/operations.rs`.
   **Do not change the visibility** of existing `pub(crate)` internal
   functions or dispatch methods. New public wrappers keep the existing
   visibility boundaries intact.

   Example — adding a `get_own_property_descriptor` method:
   ```rust
   impl JsObject {
       /// [[GetOwnProperty]] returning the raw descriptor.
       pub fn get_own_property_descriptor(
           &self,
           key: &PropertyKey,
           context: &mut Context,
       ) -> JsResult<Option<PropertyDescriptor>> {
           self.__get_own_property__(
               key,
               &mut InternalMethodPropertyContext::new(context),
           )
       }
   }
   ```

6. As a last resort, if vendoring constraints forbid any new public API
   additions, restructure the implementation to avoid needing the internal
   operation — for example, implement `SetImmutablePrototype` manually using
   only `JsObject::prototype()` (public), or skip the problematic operation
   and rely on ordinary behaviour for that internal method.

**Visible changes to vendor code that are strictly forbidden:**
- Changing `pub(crate) fn` to `pub fn` on existing functions or methods
- Changing `pub(crate)` fields on existing public structs to `pub`
- Changing `pub(crate) mod` to `pub mod` on existing modules
- Changing `pub(crate) const` to `pub const` on existing constants

These break the vendored library's internal encapsulation boundary and
create maintenance burden on future vendor updates.

The WindowProxy currently uses the transparent-proxy approach (returns the
Window directly for same-origin). When cross-origin support requires a
proper exotic object, use this methodology to add new public wrappers
without changing existing visibility boundaries.

## Related

- `content/src/webidl/README.md` — Boa platform object integration, exotic pattern
- `content/src/html/README.md` — WindowProxy, window.open, navigation split
