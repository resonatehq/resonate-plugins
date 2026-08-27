# Gotify

| | |
|---|---|
| **API** | `{base_url}` |
| **Idempotency** | No idempotency: `POST /message` accepts no client-supplied id, key or header, and every send creates a new message. Messages are locatable by `extras`: `sanitize(promise.id)` is injected as the string value of the `resonate::promise` extras key, which Gotify stores verbatim and returns from the message reads. Extras values are arbitrary JSON with no charset, length or uniqueness constraint, so the full `sanitize` yield fits; extras are not server-side filterable, so recovery is a client-side scan of the newest 200 messages of the application |
| **Reviewed by** | Claude Opus 5, 2026-08-27 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://raw.githubusercontent.com/gotify/server/v3.0.0/docs/spec.json` |
| **Self-hosted** | yes — §5 |

## 1. Address

```
gotify://[{instance}]            # omitted instance = "default"
```

## 2. Configuration

```toml
[gotify.{instance}]              # [gotify] = [gotify.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://push.example.de` |
| `client_token` | `String` | | `gtfyc.…` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Authentication](https://gotify.net/api-docs) |
| **Probe** | `GET /application` → `200` |

```
X-Gotify-Key: {client_token}
```

## 4. Operations

### 4.1 message.create

| | |
|---|---|
| **Documentation** | [Create a message](https://gotify.net/api-docs#/message/createMessage) |

```json
{ "func": "message.create", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Push one notification as one of the client token owner's applications. Resolves with the stored message once Gotify has accepted it; fan-out to connected clients is Gotify's own concern and is not observed here. Rejects application_not_found when appid names no application owned by this token's user — Gotify reports an application of another user and a nonexistent application identically — and invalid_request on any other 4xx from either request, e.g. a malformed appid on the locate request or a missing message or a wrongly typed field on the create request. Unkeyed and at-least-once: Gotify accepts no idempotency key, so a re-entry is deduplicated best-effort by scanning the newest 200 messages of the application for the stamped resonate::promise extras value; the notification is duplicated when 200 or more messages reached the application since, or when the earlier message was deleted.",
  "type": "object",
  "properties": {
    "appid": {
      "type": "integer",
      "minimum": 1,
      "description": "Id of the application to send as. Must be owned by the user the client token belongs to. Enumerate via application.list."
    },
    "message": {
      "type": "string",
      "description": "The message body. Markdown excluding HTML is allowed; whether a client renders it as markdown is decided by the client::display contentType extra."
    },
    "title": {
      "type": "string",
      "description": "The message title. The application name is used when omitted, empty or only whitespace."
    },
    "priority": {
      "type": "integer",
      "description": "Controls how clients present the message, e.g. whether the Android app plays a sound. Negative values are accepted. The application's defaultPriority is used when omitted."
    },
    "extras": {
      "type": "object",
      "description": "Extra data carried with the message, stored key-value and returned unchanged by the message reads. Keys are formatted <top-namespace>::[<sub-namespace>::]<action>; the namespaces gotify, android, ios, web, server and client are reserved for the official clients. Accepted only on JSON requests. The key resonate::promise is written by the transport and replaces any caller value under it.",
      "additionalProperties": true
    }
  },
  "required": [
    "appid",
    "message"
  ]
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body — the created message; on a recovered re-entry the matching element of the 4.1.5 scan's messages[], which carries the same Message shape."
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["application_not_found", "invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.errorDescription of the 4xx; application_not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /message → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "properties": {
    "appid": {
      "type": "integer",
      "description": "= promise.param.appid"
    },
    "message": {
      "type": "string",
      "description": "= promise.param.message"
    },
    "title": {
      "type": "string",
      "description": "= promise.param.title"
    },
    "priority": {
      "type": "integer",
      "description": "= promise.param.priority"
    },
    "extras": {
      "type": "object",
      "description": "= promise.param.extras, plus \"resonate::promise\": = sanitize(promise.id) — the injected identity",
      "additionalProperties": true
    }
  },
  "required": [
    "appid",
    "message",
    "extras"
  ]
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "External identity of the message."
    },
    "appid": {
      "type": "integer",
      "description": "Id of the application that sent the message. Derived from the token when an application token authenticates the request, otherwise the appid of the request body."
    },
    "message": {
      "type": "string",
      "description": "The message body."
    },
    "title": {
      "type": "string",
      "description": "The message title; the application name when the request omitted one."
    },
    "priority": {
      "type": "integer",
      "description": "The priority of the message; the application's defaultPriority when the request omitted one."
    },
    "extras": {
      "type": "object",
      "description": "The extras of the message, stored verbatim. Absent when the message has none. Carries resonate::promise = sanitize(promise.id) for messages this plugin created.",
      "additionalProperties": true
    },
    "date": {
      "type": "string",
      "format": "date-time",
      "description": "RFC 3339 timestamp of creation, e.g. \"2026-08-27T10:05:16.890802803Z\"."
    },
    "error": {
      "type": "string",
      "description": "General error name on an error response, e.g. \"Bad Request\", \"Unauthorized\", \"Not Found\"."
    },
    "errorCode": {
      "type": "integer",
      "description": "HTTP status of an error response."
    },
    "errorDescription": {
      "type": "string",
      "description": "Error text on an error response, e.g. \"appid not found\", \"Field 'message' is required\", \"appid is required when not authenticating with an application token\"."
    }
  },
  "required": [
    "id",
    "appid",
    "message",
    "date"
  ]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_response` |

```python
from urllib.parse import quote

import requests

SCAN = 200
STAMP = "resonate::promise"


def _check(r):
    # 401 covers a missing, unparseable or non-client token; 403 covers a
    # permission the user lacks and a session Gotify wants re-elevated.
    # Both end only when an operator issues or elevates a token.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    return {"X-Gotify-Key": cfg.client_token}


def message_create(cfg, promise):
    args = promise.param["args"]
    appid = quote(str(args["appid"]), safe="")
    token = sanitize(promise.id)

    # Locate: extras are stored verbatim and returned by the message reads,
    # so a message stamped by an earlier delivery is recoverable. Messages
    # come back newest first and 200 is the documented maximum page size, so
    # this single page is the recovery window.
    q = _check(
        requests.get(
            f"{cfg.base_url}/application/{appid}/message",
            headers=_auth(cfg),
            params={"limit": SCAN},
            timeout=10,
        )
    )
    if q.status_code == 404:
        # "application does not exist" — also the answer for an application
        # owned by another user.
        return ("rejected", {"code": "application_not_found"})
    if q.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": q.json()["errorDescription"]})
    for m in q.json()["messages"]:
        if (m.get("extras") or {}).get(STAMP) == token:
            return ("resolved", m)

    body = dict(args)
    # Extras are only accepted on an application/json request.
    body["extras"] = {**args.get("extras", {}), STAMP: token}
    r = _check(
        requests.post(
            f"{cfg.base_url}/message",
            headers=_auth(cfg) | {"Content-Type": "application/json"},
            json=body,
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["errorDescription"]})
    return ("resolved", r.json())
```

### 4.2 application.list

| | |
|---|---|
| **Documentation** | [Return all applications](https://gotify.net/api-docs#/application/getApps) |

```json
{ "func": "application.list", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "List the applications owned by the user the client token belongs to — the appid values message.create accepts, with the defaultPriority each of them applies. Takes no arguments; the whole set is returned unpaged. Rejects invalid_request on a 4xx that is not a credential failure.",
  "type": "object",
  "properties": {}
}
```

### 4.2.2 Promise Value Schema

#### Resolved

```json
{
  "type": "array",
  "description": "= response.body"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.errorDescription of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /application → 200
```

### 4.2.4 Integration Response

```json
{
  "type": ["array", "object"],
  "description": "The applications of the authenticated user; an error response returns an object carrying the error fields instead of the array.",
  "properties": {
    "error": {
      "type": "string",
      "description": "Same as 4.1.4 .error"
    },
    "errorCode": {
      "type": "integer",
      "description": "Same as 4.1.4 .errorCode"
    },
    "errorDescription": {
      "type": "string",
      "description": "Same as 4.1.4 .errorDescription"
    }
  },
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer",
        "description": "Application id; the appid message.create takes."
      },
      "name": {
        "type": "string",
        "description": "Display name of the application; also the title of a message sent without one."
      },
      "description": {
        "type": "string",
        "description": "Description of the application."
      },
      "internal": {
        "type": "boolean",
        "description": "true for an application Gotify itself owns, e.g. a plugin's; such an application should not be deleted."
      },
      "image": {
        "type": "string",
        "description": "Path of the application image relative to the Gotify base URL, e.g. \"static/defaultapp.png\"."
      },
      "defaultPriority": {
        "type": "integer",
        "description": "Priority applied to a message this application sends without one."
      },
      "createdAt": {
        "type": "string",
        "format": "date-time",
        "description": "RFC 3339 timestamp of creation."
      },
      "lastUsed": {
        "type": ["string", "null"],
        "description": "RFC 3339 timestamp of the last use of the application token; null when never used."
      },
      "sortKey": {
        "type": "string",
        "description": "Fractional index ordering the application in the clients, e.g. \"a0\"."
      },
      "token": {
        "type": "string",
        "description": "Declared by the OpenAPI document, but not returned by this endpoint from Gotify 3 on: an application token is exposed only by the create and the rotate response."
      }
    },
    "required": [
      "id",
      "name",
      "description",
      "internal",
      "image",
      "createdAt",
      "sortKey"
    ]
  }
}
```

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def application_list(cfg, promise):
    r = _check(
        requests.get(
            f"{cfg.base_url}/application",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["errorDescription"]})
    return ("resolved", r.json())
```

### 4.3 message.list

| | |
|---|---|
| **Documentation** | [Return all messages](https://gotify.net/api-docs#/message/getMessages) |

```json
{ "func": "message.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Read one page of the messages of every application the client token's user owns, newest first. Rejects invalid_request when limit is outside 1..200 or a parameter is not an integer. A plain read — not the completion mechanism; message.create observes independently.",
  "type": "object",
  "properties": {
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 200,
      "description": "Maximum number of messages to return. 100 when omitted."
    },
    "since": {
      "type": "integer",
      "minimum": 0,
      "description": "Return only messages with an id less than this value; 0 applies no filter, as does omitting it. Take it from paging.since of the previous page; the end of the walk is the absence of paging.next, never paging.since, which is 0 on the final page."
    }
  }
}
```

### 4.3.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.2.2.

### 4.3.3 Integration Request

```
GET /message{?limit,since = promise.param.*} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "paging": {
      "type": "object",
      "properties": {
        "size": {
          "type": "integer",
          "description": "Number of messages in this response."
        },
        "since": {
          "type": "integer",
          "description": "Id of the last message of this response when a further page exists; the since value of that page. 0 when there is no further page, including on a non-empty final page."
        },
        "limit": {
          "type": "integer",
          "minimum": 1,
          "maximum": 200,
          "description": "Limit applied to this request."
        },
        "next": {
          "type": "string",
          "description": "Path of the next page relative to the Gotify base URL from Gotify 3 on, e.g. \"/message?limit=1&since=2\". Absent when no further page exists."
        }
      },
      "required": [
        "size",
        "since",
        "limit"
      ]
    },
    "messages": {
      "type": "array",
      "description": "The messages, ordered by descending id.",
      "items": {
        "description": "Same as 4.1.4"
      }
    },
    "error": {
      "type": "string",
      "description": "Same as 4.1.4 .error"
    },
    "errorCode": {
      "type": "integer",
      "description": "Same as 4.1.4 .errorCode"
    },
    "errorDescription": {
      "type": "string",
      "description": "Same as 4.1.4 .errorDescription"
    }
  },
  "required": [
    "paging",
    "messages"
  ]
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def message_list(cfg, promise):
    # Following paging.next to the end is the caller's loop (one promise per
    # page), not ours.
    r = _check(
        requests.get(
            f"{cfg.base_url}/message",
            headers=_auth(cfg),
            params=promise.param["args"],
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["errorDescription"]})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM gotify/server:3.0.0
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-requests curl \
 && rm -rf /var/lib/apt/lists/*
ENV GOTIFY_DEFAULTUSER_NAME=admin \
    GOTIFY_DEFAULTUSER_PASS=admin \
    GOTIFY_SERVER_PORT=80
```

### 5.2 Run

```sh
docker rm -f plugin-gotify-test >/dev/null 2>&1 || true
docker build -t plugin-gotify-test spec/
docker run -d --name plugin-gotify-test -p 8090:80 plugin-gotify-test

while :; do
  GOTIFY_CLIENT_TOKEN=$(curl -s -u admin:admin -X POST http://localhost:8090/client \
    -H 'Content-Type: application/json' -d '{"name":"resonate"}' \
    | python3 -c 'import json,sys; print(json.load(sys.stdin).get("token",""))' 2>/dev/null)
  if [ -n "$GOTIFY_CLIENT_TOKEN" ] \
     && curl -sf -o /dev/null -H "X-Gotify-Key: $GOTIFY_CLIENT_TOKEN" \
        http://localhost:8090/application; then
    break
  fi
  sleep 2
done

GOTIFY_FIXTURE_OK=$(curl -sf -u admin:admin -X POST http://localhost:8090/application \
  -H 'Content-Type: application/json' -d '{"name":"resonate-ok","description":"owned by admin"}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
curl -sf -u admin:admin -X POST http://localhost:8090/user \
  -H 'Content-Type: application/json' \
  -d '{"name":"other","pass":"otherpass","admin":false}' >/dev/null
GOTIFY_FIXTURE_FAIL=$(curl -sf -u other:otherpass -X POST http://localhost:8090/application \
  -H 'Content-Type: application/json' -d '{"name":"resonate-fail","description":"owned by other"}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')

export GOTIFY_BASE_URL=http://localhost:8090
export GOTIFY_CLIENT_TOKEN
export GOTIFY_FIXTURE_OK
export GOTIFY_FIXTURE_FAIL
```
