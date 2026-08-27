# Zendesk

| | |
|---|---|
| **API** | `https://{subdomain}.zendesk.com/api/v2` |
| **Idempotency** | `Idempotency-Key` header on ticket create — 2h window, cached-response replay; same key with a different body → `400` |
| **Reviewed by** | Claude Opus, 2026-08-26 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://developer.zendesk.com/zendesk/oas.yaml` |
| **Self-hosted** | no — SaaS only, no §5 |

## 1. Address

```
zendesk://[{instance}]           # omitted instance = "default"
```

## 2. Configuration

```toml
[zendesk.{instance}]             # [zendesk] = [zendesk.default]
```

| key | type | default | example |
|---|---|---|---|
| `subdomain` | `String` | = `instance` | `acme` |
| `email` | `String` | | `bot@acme.com` |
| `api_token` | `String` | | `…` |
| `poll` | `Duration` | `15m` | `15m` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Security and authentication](https://developer.zendesk.com/api-reference/introduction/security-and-auth/#api-token) |
| **Probe** | `GET /api/v2/ticket_fields` → `200` |

```
Authorization: Basic base64("{email}/token:{api_token}")
```

## 4. Operations

### 4.1 ticket.create

| | |
|---|---|
| **Documentation** | [Create Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#create-ticket) |

```json
{ "func": "ticket.create", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Open a support ticket. The promise resolves when the ticket reaches its frozen terminal state (closed); it rejects if the ticket is deleted before closing. Days-scale: solved tickets auto-close after at most 28 days — size timeoutAt beyond that.",
  "type": "object",
  "properties": {
    "comment": {
      "type": "object",
      "description": "First message on the ticket.",
      "properties": {
        "body": {
          "type": "string"
        },
        "html_body": {
          "type": "string"
        },
        "public": {
          "type": "boolean",
          "description": "false = internal note"
        }
      }
    },
    "subject": {
      "type": "string",
      "description": "Ticket subject line."
    },
    "type": {
      "type": "string",
      "enum": [
        "question",
        "incident",
        "problem",
        "task"
      ]
    },
    "priority": {
      "type": "string",
      "enum": [
        "urgent",
        "high",
        "normal",
        "low"
      ]
    },
    "requester": {
      "type": "object",
      "description": "End user the ticket is for; created on the fly if unknown.",
      "properties": {
        "name": {
          "type": "string"
        },
        "email": {
          "type": "string"
        }
      }
    },
    "group_id": {
      "type": "integer",
      "description": "Assigned group. Enumerate via GET /api/v2/groups."
    },
    "tags": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "custom_fields": {
      "type": "array",
      "description": "[{\"id\": <field id>, \"value\": ...}]; field ids via GET /api/v2/ticket_fields.",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer"
          },
          "value": {}
        },
        "required": [
          "id"
        ]
      }
    }
  },
  "required": [
    "comment"
  ]
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "= response.body.ticket.id"
    },
    "status": {
      "type": "string",
      "description": "= response.body.ticket.status"
    },
    "subject": {
      "type": "string",
      "description": "= response.body.ticket.subject"
    },
    "tags": {
      "type": "array",
      "description": "= response.body.ticket.tags"
    },
    "created_at": {
      "type": "string",
      "description": "= response.body.ticket.created_at"
    },
    "updated_at": {
      "type": "string",
      "description": "= response.body.ticket.updated_at"
    }
  }
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request", "deleted"]
    },
    "detail": {
      "description": "invalid_request: = response.body of the 4xx; deleted: absent (the 404 has no body worth keeping)"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/v2/tickets → 201
Idempotency-Key: {sanitize(promise.id)}
```

```json
{
  "type": "object",
  "properties": {
    "ticket": {
      "type": "object",
      "properties": {
        "comment": {
          "type": "object",
          "description": "= promise.param.comment"
        },
        "subject": {
          "type": "string",
          "description": "= promise.param.subject"
        },
        "type": {
          "type": "string",
          "description": "= promise.param.type"
        },
        "priority": {
          "type": "string",
          "description": "= promise.param.priority"
        },
        "requester": {
          "type": "object",
          "description": "= promise.param.requester"
        },
        "group_id": {
          "type": "integer",
          "description": "= promise.param.group_id"
        },
        "tags": {
          "type": "array",
          "description": "= promise.param.tags"
        },
        "custom_fields": {
          "type": "array",
          "description": "= promise.param.custom_fields"
        },
        "external_id": {
          "type": "string",
          "description": "= sanitize(promise.id) (not unique-enforced: the Idempotency-Key header is the dedup, this field is the lookup)"
        }
      },
      "required": [
        "comment",
        "external_id"
      ]
    }
  },
  "required": [
    "ticket"
  ]
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "ticket": {
      "type": "object",
      "properties": {
        "id": {
          "type": "integer",
          "description": "External identity. Record durably."
        },
        "status": {
          "type": "string",
          "enum": [
            "new",
            "open",
            "pending",
            "hold",
            "solved",
            "closed"
          ]
        },
        "external_id": {
          "type": "string",
          "description": "Echo of sanitize(promise.id)."
        },
        "subject": {
          "type": "string"
        },
        "tags": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "url": {
          "type": "string",
          "description": "API URL of the ticket (JSON, not the agent UI)."
        },
        "created_at": {
          "type": "string",
          "description": "ISO 8601"
        },
        "updated_at": {
          "type": "string",
          "description": "ISO 8601"
        }
      }
    }
  },
  "required": [
    "ticket"
  ]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
import time

import requests


def _check(r):
    # 401 token rejected, 403 role lacks permission — an operator must act.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    if r.status_code >= 400:
        raise Exception("release", r.text)


def ticket_create(cfg, promise):
    base = f"https://{cfg.subdomain}.zendesk.com/api/v2"
    auth = (f"{cfg.email}/token", cfg.api_token)
    args = promise.param["args"]

    # Locate our ticket by the stamped external_id: on re-entry this
    # recovers the ticket a previous attempt created.
    r = requests.get(
        f"{base}/tickets",
        auth=auth,
        params={"external_id": sanitize(promise.id)},
        timeout=10,
    )
    _check(r)
    hits = r.json()["tickets"]

    if hits:
        tid = min(t["id"] for t in hits)  # external_id is not unique: pick deterministically
    else:
        # The Idempotency-Key makes a duplicate POST inside the 2h window
        # a safe replay.
        r = requests.post(
            f"{base}/tickets",
            auth=auth,
            headers={"Idempotency-Key": sanitize(promise.id)},
            json={"ticket": args | {"external_id": sanitize(promise.id)}},
            timeout=10,
        )
        if r.status_code in (400, 404, 422):
            return ("rejected", {"code": "invalid_request", "detail": r.json()})
        _check(r)
        tid = r.json()["ticket"]["id"]

    # "solved" is NOT terminal — it reopens on customer reply; only
    # "closed" is frozen.
    while True:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        r = requests.get(f"{base}/tickets/{tid}", auth=auth, timeout=10)
        if r.status_code == 404:
            return ("rejected", {"code": "deleted"})  # soft-deleted before closing
        _check(r)
        t = r.json()["ticket"]
        if t["status"] == "closed":
            break
        time.sleep(cfg.poll.total_seconds())

    keys = ("id", "status", "subject", "tags", "created_at", "updated_at")
    return ("resolved", {k: t[k] for k in keys})  # the 4.1.2 Resolved mapping
```

### 4.2 ticket.comment

| | |
|---|---|
| **Documentation** | [Update Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#update-ticket) |

```json
{ "func": "ticket.comment", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Add a comment to a live ticket. Unkeyed and at-least-once: a re-entry may duplicate the comment; dedup is a best-effort textual scan of the newest 100 comments (false positives and negatives possible). Rejects permanently on a closed ticket (closed tickets are frozen).",
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "Ticket id."
    },
    "comment": {
      "type": "object",
      "properties": {
        "body": {
          "type": "string"
        },
        "public": {
          "type": "boolean",
          "description": "false = internal note"
        }
      },
      "required": [
        "body"
      ]
    }
  },
  "required": [
    "id",
    "comment"
  ]
}
```

### 4.2.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body.ticket"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found", "closed", "invalid_request"]
    },
    "detail": {
      "description": "closed / invalid_request: = response.body of the 422/400; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
PUT /api/v2/tickets/{promise.param.id} → 200
```

```json
{
  "type": "object",
  "properties": {
    "ticket": {
      "type": "object",
      "properties": {
        "comment": {
          "type": "object",
          "description": "= promise.param.comment"
        }
      },
      "required": [
        "comment"
      ]
    }
  },
  "required": [
    "ticket"
  ]
}
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_response` |

```python
def ticket_comment(cfg, promise):
    base = f"https://{cfg.subdomain}.zendesk.com/api/v2"
    auth = (f"{cfg.email}/token", cfg.api_token)
    args = promise.param["args"]

    # The PUT is unkeyed: best-effort dedup — scan the newest 100 comments
    # for an identical body before re-PUTting.
    r = requests.get(
        f"{base}/tickets/{args['id']}/comments",
        auth=auth,
        params={"sort_order": "desc", "per_page": 100},
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    _check(r)
    landed = any(c["body"] == args["comment"]["body"] for c in r.json()["comments"])

    if not landed:
        r = requests.put(
            f"{base}/tickets/{args['id']}",
            auth=auth,
            json={"ticket": {"comment": args["comment"]}},
            timeout=10,
        )
        if r.status_code == 404:
            return ("rejected", {"code": "not_found"})
        if r.status_code in (400, 422):
            body = r.json()
            # 422 covers all validation failures; "closed" only when the
            # frozen-ticket signal is present.
            closed = "closed" in str(body.get("details", "")).lower()
            return ("rejected", {"code": "closed" if closed else "invalid_request", "detail": body})
        _check(r)
        return ("resolved", r.json()["ticket"])

    # Landed previously: resolve with the current record.
    r = requests.get(f"{base}/tickets/{args['id']}", auth=auth, timeout=10)
    _check(r)
    return ("resolved", r.json()["ticket"])
```

### 4.3 ticket.get

| | |
|---|---|
| **Documentation** | [Show Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#show-ticket) |

```json
{ "func": "ticket.get", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Read one ticket. Rejects on 404 (deleted or never existed). A plain read — not the completion mechanism; ticket.create observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "Ticket id."
    }
  },
  "required": [
    "id"
  ]
}
```

### 4.3.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body.ticket"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found"]
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/v2/tickets/{promise.param.id} → 200
```

### 4.3.4 Integration Response

Same as 4.1.4.

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def ticket_get(cfg, promise):
    base = f"https://{cfg.subdomain}.zendesk.com/api/v2"
    auth = (f"{cfg.email}/token", cfg.api_token)
    tid = promise.param["args"]["id"]

    r = requests.get(f"{base}/tickets/{tid}", auth=auth, timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    _check(r)
    return ("resolved", r.json()["ticket"])
```

