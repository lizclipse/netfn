# API Interface

This is the API interface definition for how `netfn` expects calls and messaging to work.
By defining an interface and using it as the source of truth, it allows the library to have an ideal
to work towards (and test against), as well as a way to expand into other languages down-the-line.

Each message schema is defined using TypeScript, as it allows very specific but understandable
syntax.
Some 'custom' types are used, but only to be more specific where TypeScript is not (such as ints).

## Call-response

For things like HTTP, the nature of the transport only truly allows for a simple call-response.
This kind of thing can work for for simple function calls, but not for any sort of
bi-directional stream, which is possible via the [tunnel interface](#Tunnel).
A simple transport like HTTP does, however, allow for simple scratch testing when learning
an API in the first place, so offering one makes it easier to onboard new devs even if
a client library is provided.
It also allows for 1-off requests to be made without the overhead of opening a full tunnel.

### Request

Each request has to specify the target service, the function being called, and all of
the arguments in the function that are required.

```ts
interface CallResponseRequest {
  service: string; // Using an adjacently-tagged representation
  call: {
    fn: string; // Using an adjacently-tagged representation,
    // Args is an object to allow optional field to be omitted and to enforce ordering.
    // By using an object and string keys, it allows implementations to define the arguments in
    // terms of structs, simplifying implementation and reducing the need for custom JSON (de)serialising.
    args: Record<string, any>;
  }
}
```

Example, using JSON:

```jsonc
{
  "service": "TestService",
  "call": {
    "fn": "test_fn",
    "args": {
      "0": "first argument",
      "1": 2,
      "2": { "foo": "bar" },
      "4": ["a", "b", "c"]
    }
  }
}
```

### Response

As call-response transports implicitly link the response to the request that was made,
requests are simply the exact result object, without any extra wrapping object.

### Errors

```ts
interface GenericError {
  code: string; // Standard code useful for things like i18n
  message: string; // Simple error message for debugging
}
```

Example, using JSON:

```jsonc
{
  "code": "...",
  "message": "..."
}
```

### HTTP

#### Endpoint

Any endpoint is allowed, such as:

```
/api/v1
```

All requests are made to this endpoint, with the contents determining what function is called.

#### Headers

Only the `Content-Type` header is required, all others are up to the server implementors
to use how they see fit.
All implementors have to support JSON, other encodings can be supported as needed or wanted.
MessagePack is a good option to support, as it will then match the supported encodings of WebSocket
tunnels.

## Tunnel

A bi-directional transport allows for both simple function calls as well as streams that
work in either direction or both. However, tunnels tend to require more work to set up and use,
whereas a more simple transport such as HTTP allows for scratch testing when learning the API.
Most clients should default to a tunnel transport if available, but servers should provide
a HTTP transport for scratch-testing.

### Function calls

#### Request

Inside a tunnel, the requests look very similar to call-response requests, and this is intentional.
By keeping them identical, except for additional routing fields, it allows the server to process the
requests using mostly the same systems.

```ts
interface TunnelRequest extends CallResponseRequest {
  type: "request";
  // Refs are used to make sure the responses can be tied together properly.
  // Each ref value must not be reused, so a simple incrementing integer is ideal.
  // If this integer reaches the max size (or near), then the tunnel much be re-opened.
  // Both client and server can do this in order to allow language differences to be taken into account.
  // If a client does reuse a value, then the server should follow, as it is only the client that will
  // risk confusing responses.
  ref: u64;
}
```

Example, using JSON:

```jsonc
{
  "type": "request",
  "service": "TestService",
  "call": {
    "fn": "test_fn",
    "args": {
      "0": "first argument",
      "1": 2,
      "2": { "foo": "bar" },
      "4": ["a", "b", "c"]
    }
  },
  "ref": 0
}
```

#### Response

To keep continuity, a `ref` field is used to allow linking the request and response together.
If a response is sent to an unknown ref, the client may drop it silently, but it is recommended
to at least log the event.

```ts
interface TunnelResponse {
  type: "response";
  ref: u64;
  data: any; // This is the direct result of the called fn
}
```

Example, using JSON:

```jsonc
{
  "type": "response",
  "data": {
    // Response obj
  },
  "ref": 0
}
```

### Streams

Stream messages inside a tunnel are separate from call-reponse messages to keep them clear and make
routing more simple.
As they are completely separate, call `fn`s may be overloaded to provide both call-response and stream
handling, allowing the same call to be available in a call-response only transport and as a stream.
While these handlers _may_ have different arguments, it is recommended that servers provide the same
to reduce confusion when in use.

#### Stream open

```ts
interface TunnelStreamOpen extends CallResponseRequest {
  type: "stream_open";
  // Refs are used to make sure the ready responses can be tied together properly.
  // As stream messages are separate from call-response ones, these refs _may_ be based on a different
  // count to call-response refs, however it is recommended to not do this in clients to aid with debugging.
  // Servers must handle both styles, and the same max-size reopen catch applies.
  // If a client does reuse a value, then the server should follow, as it is only the client that will
  // risk confusing responses.
  ref: u64;
}
```

Example, using JSON:

```jsonc
{
  "type": "stream_open",
  "service": "TestService",
  "call": {
    "fn": "test_fn",
    "args": {
      "0": "first argument",
      "1": 2,
      "2": { "foo": "bar" },
      "4": ["a", "b", "c"]
    }
  },
  "ref": 0
}
```

#### Stream opened

```ts
interface TunnelStreamReady {
  type: "stream_ready";
  ref: u64;
  // Handles are a different count to refs, so overlaps are expected, but each handle must be a
  // different value.
  // The same max-size reopen catch applies.
  // If a client does reuse a value, then the server should follow, as it is only the client that will
  // risk confusing responses.
  handle: u64;
}
```

Example, using JSON:

```jsonc
{
  "type": "stream_ready",
  "ref": 0,
  "handle": 1
}
```

#### Message

Stream messages may be set in either direction, however if a side isn't expecting messages then
the other must send an error on the same handle to inform the other side that this stream does not
exist and to clean up any handlers.

```ts
interface TunnelMessage {
  type: "stream_message";
  handle: u64;
  data: any;
}
```

Example, using JSON:

```jsonc
{
  "type": "stream_message",
  "handle": 1,
  "data": {
    // Data object
  }
}
```

#### Close

Streams may be closed by either side, and any messages set to this stream will be silently dropped
after this point.
If an unknown or already closed stream is attempted to be closed, then this is ignored.

```ts
interface TunnelStreamClose {
  type: "stream_close";
  handle: u64;
}
```

Example, using JSON:

```jsonc
{
  "type": "stream_close",
  "handle": 0
}
```

### Errors

#### Call Error

Errors are counted as responses to a call.
An error for an unknown call may be ignored.

```ts
interface TunnelCallError {
  type: "error";
  ref: u64;
  error: GenericError;
}
```

Example, using JSON:

```jsonc
{
  "type": "error",
  "ref": 2,
  "error": {
    "code": "...",
    "message": "..."
  }
}
```

#### Stream Error

A stream error will implicitly close the stream.
An error for an unknown stream may be ignored.

```ts
interface TunnelStreamError {
  type: "stream_error";
  handle: u64;
  error: GenericError;
}
```

Example, using JSON:

```jsonc
{
  "type": "stream_error",
  "handle": 2,
  "error": {
    "code": "...",
    "message": "..."
  }
}
```

#### Stream Open Error

If an errors occurs while the stream is opening, then a dedicated response is needed.

```ts
interface TunnelStreamOpenError {
  type: "stream_open_error";
  ref: u64;
  error: GenericError;
}
```

Example, using JSON:

```jsonc
{
  "type": "stream_open_error",
  "ref": 2,
  "error": {
    "code": "...",
    "message": "..."
  }
}
```


### WS

WebSockets allow for both text and binary messages, which can both be used.
Text messages are expected to be JSON, and binary ones MessagePack.
Implementors may use the headers and query params how they see fit.
