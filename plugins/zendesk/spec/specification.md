# Zendesk

| | |
|---|---|
| **API** | `https://{subdomain}.zendesk.com/api/v2` |
| **Idempotency** | `Idempotency-Key: <key>` on `POST /api/v2/tickets`. Any string up to 255 characters, scoped to the account; stored for 2 hours. A replay inside the window returns the original response and the header `x-idempotency-lookup: hit` (a first use answers `miss`); the same key with a different body answers `400` "Request parameters don't match the given idempotency key". `sanitize(promise.id)` (17–117 characters, `[A-Za-z0-9._-]`) satisfies the constraint. `PUT /api/v2/tickets/{ticket_id}`, `DELETE /api/v2/tickets/{ticket_id}` and `POST /api/v2/tickets/{ticket_id}/merge` accept no idempotency key |
| **Reviewed by** | Claude Opus 5, 2026-08-29 |

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
| `subdomain` | `String` | `= instance` | `acme` |
| `email` | `String` | | `agent@acme.com` |
| `api_token` | `String` | | `…` |
| `poll` | `Duration` | `2s` | `2s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Security and authentication](https://developer.zendesk.com/api-reference/introduction/security-and-auth/) |
| **Probe** | `GET /api/v2/tickets/count` → `200` |

```
Authorization: Basic {base64("{email}/token:{api_token}")}
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
  "description": "Create one ticket. Resolves on the 201 with the created ticket record; the ticket is created, not worked — its status is \"new\" and the promise does not wait for an agent. The promise identity is sent as the Idempotency-Key and stamped on the ticket's external_id, so a re-delivery inside the 2 hour key window returns the original ticket, and a re-delivery after it finds the ticket by external_id instead of creating a second one; external_id is therefore reserved and cannot be set by the caller. Rejects invalid_request if the request body is malformed or the ticket fails validation (unknown requester_id, assignee_id, group_id or custom field id, a custom field value the field rejects, a status the account does not allow).",
  "type": "object",
  "properties": {
    "subject": {
      "type": "string",
      "description": "The subject line of the ticket."
    },
    "comment": {
      "type": "object",
      "description": "The first comment on the ticket; its body becomes the ticket's read-only description.",
      "properties": {
        "body": {
          "type": "string",
          "description": "The comment as plain text."
        },
        "html_body": {
          "type": "string",
          "description": "The comment as HTML. Set body or html_body, not both."
        },
        "public": {
          "type": "boolean",
          "description": "true for a public comment, false for an internal note. true when omitted; the value set here persists for later comments unless changed."
        },
        "author_id": {
          "type": "integer",
          "description": "Author of the comment. Defaults to the authenticated user."
        },
        "uploads": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "Upload tokens returned by POST /api/v2/uploads, attaching those files to this comment."
        }
      }
    },
    "requester_id": {
      "type": "integer",
      "description": "The user who requested the ticket. Defaults to the authenticated user. Enumerate via GET /api/v2/users/search."
    },
    "requester": {
      "type": "object",
      "description": "Creates the requester as a new end user when no user with this email exists. Mutually exclusive with requester_id.",
      "properties": {
        "name": {
          "type": "string"
        },
        "email": {
          "type": "string"
        },
        "locale_id": {
          "type": "integer"
        }
      }
    },
    "submitter_id": {
      "type": "integer",
      "description": "The user recorded as submitting the ticket. The submitter is the author of the first comment."
    },
    "assignee_id": {
      "type": "integer",
      "description": "The agent the ticket is assigned to."
    },
    "assignee_email": {
      "type": "string",
      "description": "The email address of the agent to assign the ticket to. Alternative to assignee_id."
    },
    "group_id": {
      "type": "integer",
      "description": "The group the ticket is assigned to. Enumerate via GET /api/v2/groups."
    },
    "organization_id": {
      "type": "integer",
      "description": "The requester's organization. Only an organization the requester belongs to is accepted."
    },
    "priority": {
      "type": "string",
      "enum": ["urgent", "high", "normal", "low"],
      "description": "The urgency with which the ticket should be addressed."
    },
    "status": {
      "type": "string",
      "enum": ["new", "open", "pending", "hold", "solved", "closed"],
      "description": "The state of the ticket. On an account with custom ticket statuses activated this is the status category of custom_status_id."
    },
    "custom_status_id": {
      "type": "integer",
      "description": "The custom ticket status, on an account with custom ticket statuses activated. Enumerate via GET /api/v2/custom_statuses."
    },
    "type": {
      "type": "string",
      "enum": ["problem", "incident", "question", "task"],
      "description": "The type of the ticket. A ticket of type \"task\" can carry due_at; a ticket of type \"incident\" can carry problem_id."
    },
    "tags": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The tags applied to the ticket. This is a set operation: the array replaces any tags the ticket would otherwise have."
    },
    "custom_fields": {
      "type": "array",
      "description": "Values for the account's custom ticket fields. Enumerate the field ids via GET /api/v2/ticket_fields.",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer",
            "description": "The custom field's id."
          },
          "value": {
            "description": "The value, of the type the field declares."
          }
        },
        "required": ["id", "value"]
      }
    },
    "due_at": {
      "type": ["string", "null"],
      "description": "ISO 8601. Only meaningful on a ticket of type \"task\"."
    },
    "collaborator_ids": {
      "type": "array",
      "items": {
        "type": "integer"
      },
      "description": "Ids of the users CC'd on the ticket."
    },
    "email_ccs": {
      "type": "array",
      "description": "Agents or end users to add as email CCs.",
      "items": {
        "type": "object",
        "properties": {
          "action": {
            "type": "string",
            "enum": ["put", "delete"]
          },
          "user_id": {
            "type": "string"
          },
          "user_email": {
            "type": "string"
          },
          "user_name": {
            "type": "string"
          }
        }
      }
    },
    "followers": {
      "type": "array",
      "description": "Agents to add as followers.",
      "items": {
        "type": "object",
        "properties": {
          "action": {
            "type": "string",
            "enum": ["put", "delete"]
          },
          "user_id": {
            "type": "string"
          },
          "user_email": {
            "type": "string"
          }
        }
      }
    },
    "recipient": {
      "type": "string",
      "description": "The original recipient email address of the ticket. Notification emails are sent from this address."
    },
    "problem_id": {
      "type": "integer",
      "description": "For a ticket of type \"incident\", the id of the problem ticket it is linked to."
    },
    "brand_id": {
      "type": "integer",
      "description": "Enterprise only. The brand the ticket belongs to."
    },
    "ticket_form_id": {
      "type": "integer",
      "description": "Enterprise only. The ticket form to render for the ticket."
    },
    "macro_ids": {
      "type": "array",
      "items": {
        "type": "integer"
      },
      "description": "Macro ids to record in the ticket audit."
    },
    "via_followup_source_id": {
      "type": "integer",
      "description": "The id of a closed ticket this ticket follows up on."
    }
  },
  "required": ["comment"]
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body.ticket, or = response.body.tickets[0] of the external_id lookup when the ticket was recovered rather than created"
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
      "description": "invalid_request: = response.body of the 4xx as text ({error, description, details})"
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
        "subject": {
          "type": "string",
          "description": "= promise.param.subject"
        },
        "comment": {
          "type": "object",
          "description": "= promise.param.comment"
        },
        "requester_id": {
          "type": "integer",
          "description": "= promise.param.requester_id"
        },
        "requester": {
          "type": "object",
          "description": "= promise.param.requester"
        },
        "submitter_id": {
          "type": "integer",
          "description": "= promise.param.submitter_id"
        },
        "assignee_id": {
          "type": "integer",
          "description": "= promise.param.assignee_id"
        },
        "assignee_email": {
          "type": "string",
          "description": "= promise.param.assignee_email"
        },
        "group_id": {
          "type": "integer",
          "description": "= promise.param.group_id"
        },
        "organization_id": {
          "type": "integer",
          "description": "= promise.param.organization_id"
        },
        "priority": {
          "type": "string",
          "description": "= promise.param.priority"
        },
        "status": {
          "type": "string",
          "description": "= promise.param.status"
        },
        "custom_status_id": {
          "type": "integer",
          "description": "= promise.param.custom_status_id"
        },
        "type": {
          "type": "string",
          "description": "= promise.param.type"
        },
        "tags": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "= promise.param.tags"
        },
        "custom_fields": {
          "type": "array",
          "description": "= promise.param.custom_fields"
        },
        "due_at": {
          "type": ["string", "null"],
          "description": "= promise.param.due_at"
        },
        "collaborator_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          },
          "description": "= promise.param.collaborator_ids"
        },
        "email_ccs": {
          "type": "array",
          "description": "= promise.param.email_ccs"
        },
        "followers": {
          "type": "array",
          "description": "= promise.param.followers"
        },
        "recipient": {
          "type": "string",
          "description": "= promise.param.recipient"
        },
        "problem_id": {
          "type": "integer",
          "description": "= promise.param.problem_id"
        },
        "brand_id": {
          "type": "integer",
          "description": "= promise.param.brand_id"
        },
        "ticket_form_id": {
          "type": "integer",
          "description": "= promise.param.ticket_form_id"
        },
        "macro_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          },
          "description": "= promise.param.macro_ids"
        },
        "via_followup_source_id": {
          "type": "integer",
          "description": "= promise.param.via_followup_source_id"
        },
        "external_id": {
          "type": "string",
          "description": "= sanitize(promise.id)"
        }
      },
      "required": ["comment", "external_id"]
    }
  },
  "required": ["ticket"]
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
          "description": "Assigned when the ticket is created."
        },
        "url": {
          "type": "string",
          "description": "The API url of this ticket."
        },
        "external_id": {
          "type": ["string", "null"],
          "description": "The client-supplied id linking the ticket to a local record."
        },
        "created_at": {
          "type": "string",
          "description": "ISO 8601."
        },
        "updated_at": {
          "type": "string",
          "description": "ISO 8601. Advances only on an update that generates a ticket event."
        },
        "generated_timestamp": {
          "type": "integer",
          "description": "Unix timestamp of the last update, including system updates."
        },
        "type": {
          "type": ["string", "null"],
          "enum": ["problem", "incident", "question", "task", null]
        },
        "subject": {
          "type": ["string", "null"]
        },
        "raw_subject": {
          "type": ["string", "null"],
          "description": "The dynamic content placeholder, if present, or the subject value."
        },
        "description": {
          "type": "string",
          "description": "The first comment on the ticket. Read-only."
        },
        "priority": {
          "type": ["string", "null"],
          "enum": ["urgent", "high", "normal", "low", null]
        },
        "status": {
          "type": "string",
          "enum": ["new", "open", "pending", "hold", "solved", "closed"],
          "description": "\"closed\" is terminal: a closed ticket is locked and can no longer be updated, commented on, merged into, or tagged. \"solved\" is not terminal — a customer reply reopens the ticket, and an account automation closes a solved ticket later. On an account with custom ticket statuses activated this is the status category of custom_status_id."
        },
        "custom_status_id": {
          "type": ["integer", "null"],
          "description": "The custom ticket status, on an account with custom ticket statuses activated."
        },
        "recipient": {
          "type": ["string", "null"]
        },
        "requester_id": {
          "type": "integer"
        },
        "submitter_id": {
          "type": "integer"
        },
        "assignee_id": {
          "type": ["integer", "null"]
        },
        "organization_id": {
          "type": ["integer", "null"]
        },
        "group_id": {
          "type": ["integer", "null"]
        },
        "collaborator_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          }
        },
        "follower_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          }
        },
        "email_cc_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          }
        },
        "forum_topic_id": {
          "type": ["integer", "null"]
        },
        "problem_id": {
          "type": ["integer", "null"]
        },
        "has_incidents": {
          "type": "boolean",
          "description": "true on a problem ticket with linked incidents."
        },
        "is_public": {
          "type": "boolean",
          "description": "true if any comment on the ticket is public."
        },
        "due_at": {
          "type": ["string", "null"],
          "description": "ISO 8601. Present on a ticket of type \"task\"."
        },
        "tags": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "custom_fields": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "id": {
                "type": "integer"
              },
              "value": {
                "description": "The value, of the type the field declares; null when unset."
              }
            }
          }
        },
        "satisfaction_rating": {
          "type": ["object", "null"],
          "additionalProperties": true,
          "description": "The rating, or the state of satisfaction (\"offered\", \"unoffered\"). null on plans without CSAT."
        },
        "sharing_agreement_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          }
        },
        "followup_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          },
          "description": "Ids of follow-up tickets. Populated only once the ticket is closed."
        },
        "ticket_form_id": {
          "type": ["integer", "null"],
          "description": "Enterprise only."
        },
        "brand_id": {
          "type": ["integer", "null"],
          "description": "Enterprise only."
        },
        "allow_channelback": {
          "type": "boolean"
        },
        "allow_attachments": {
          "type": "boolean"
        },
        "from_messaging_channel": {
          "type": "boolean"
        },
        "via": {
          "type": "object",
          "properties": {
            "channel": {
              "type": "string",
              "description": "How the ticket was created, e.g. \"web\", \"api\", \"email\", \"rule\", \"system\"."
            },
            "source": {
              "type": "object",
              "additionalProperties": true
            }
          }
        }
      },
      "required": ["id", "url", "created_at", "updated_at", "status", "requester_id", "description", "tags", "custom_fields", "via"]
    }
  },
  "required": ["ticket"]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_response` |

```python
import base64
import time
from urllib.parse import quote

import requests

JOB_TERMINAL = ("completed", "failed", "killed")


def _api(cfg):
    return f"https://{cfg.subdomain}.zendesk.com/api/v2"


def _auth(cfg):
    basic = base64.b64encode(f"{cfg.email}/token:{cfg.api_token}".encode()).decode()
    return {"Authorization": f"Basic {basic}"}


def _check(r):
    # 401 "Couldn't authenticate you" and 403 (the agent's role, or the token's
    # scope, does not permit the action) end only when an operator acts.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    # Every documented Zendesk limit answers 429 with Retry-After and resets on
    # its own: the account limit per minute, and the per-endpoint limits (30
    # updates per 10 minutes per user per ticket, 400 ticket deletions per
    # minute, 100 search exports per minute).
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _query(args, keys):
    params = {}
    for k in keys:
        if k not in args:
            continue
        v = args[k]
        if isinstance(v, bool):
            params[k] = "true" if v else "false"
        elif isinstance(v, list):
            params[k] = ",".join(str(x) for x in v)
        else:
            params[k] = v
    return params


def ticket_create(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    key = sanitize(promise.id)

    # The Idempotency-Key is honoured for two hours only; past that window the
    # external_id stamped below is the sole handle on the ticket this promise
    # created. Filtering by external_id is a list query, not a unique lookup:
    # Zendesk does not enforce uniqueness on external_id. The lookup costs one
    # request on every delivery, the first included, and pays only past that
    # window.
    r = _check(requests.get(f"{api}/tickets", headers=auth,
                            params={"external_id": key}, timeout=10))
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    found = r.json().get("tickets") or []
    if found:
        return ("resolved", found[0])

    r = requests.post(
        f"{api}/tickets",
        headers={**auth, "Idempotency-Key": key},
        json={"ticket": dict(args, external_id=key)},
        timeout=10,
    )
    _check(r)
    if r.status_code >= 400:
        # 400 ParameterMissing for a malformed body, 422 RecordInvalid for a
        # field the account rejects.
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json()["ticket"])
```

### 4.2 ticket.get

| | |
|---|---|
| **Documentation** | [Show Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#show-ticket) |

```json
{ "func": "ticket.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one ticket. Resolves with the ticket record; the record does not carry the ticket's comments, which ticketcomment.list reads. Rejects not_found if no ticket with this id is visible to the credentials, which is also the answer for a ticket that has been deleted, and invalid_request if the request fails validation (an unknown sideload name in include).",
  "type": "object",
  "properties": {
    "ticket_id": {
      "type": "integer",
      "description": "The ticket's id. Enumerate via ticket.list or ticket.search."
    },
    "include": {
      "type": "string",
      "description": "Comma-separated sideloads: users, groups, organizations, metric_sets and slas are returned each as its own top-level array in the response, comment_count as a property added to the ticket object itself."
    }
  },
  "required": ["ticket_id"]
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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "not_found: = response.body of the 404 as text ({error: \"RecordNotFound\", description}); invalid_request: = response.body of the 4xx as text"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/v2/tickets/{promise.param.ticket_id}{?include = promise.param.*} → 200
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def ticket_get(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    ticket_id = quote(str(args["ticket_id"]), safe="")

    r = requests.get(f"{api}/tickets/{ticket_id}", headers=auth,
                     params=_query(args, ("include",)), timeout=10)
    # A deleted ticket answers 404 here; it stays listed by
    # GET /api/v2/deleted_tickets until it is purged.
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json()["ticket"])
```

### 4.3 ticket.list

| | |
|---|---|
| **Documentation** | [List Tickets](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#list-tickets) |

```json
{ "func": "ticket.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "List one page of the account's tickets, newest ids last, optionally filtered to one external_id. Resolves with the page and its cursors: meta.has_more says whether another page follows, meta.after_cursor is the value to pass as page[after] on the next call, and meta.before_cursor the value to pass as page[before] to walk back. Rejects invalid_request if the request fails validation (a malformed or expired cursor, an unknown sideload name in include).",
  "type": "object",
  "properties": {
    "external_id": {
      "type": "string",
      "description": "Return only tickets carrying this external id. External ids are not unique, so several tickets may answer."
    },
    "include": {
      "type": "string",
      "description": "Comma-separated sideloads: users, groups, organizations, metric_sets and slas are returned each as its own top-level array in the response, comment_count as a property added to each ticket object itself."
    },
    "page[size]": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100,
      "description": "Tickets per page, up to 100. Supplying it selects cursor pagination."
    },
    "page[after]": {
      "type": "string",
      "description": "The meta.after_cursor of the previous page."
    },
    "page[before]": {
      "type": "string",
      "description": "The meta.before_cursor of the previous page, walking backward."
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

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body of the 4xx as text"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/v2/tickets{?external_id,include,page[size],page[after],page[before] = promise.param.*} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "tickets": {
      "type": "array",
      "items": {
        "description": "Same as 4.1.4 .ticket"
      }
    },
    "meta": {
      "type": "object",
      "description": "Present when page[size] was supplied.",
      "properties": {
        "has_more": {
          "type": "boolean",
          "description": "false on the last page."
        },
        "after_cursor": {
          "type": ["string", "null"]
        },
        "before_cursor": {
          "type": ["string", "null"],
          "description": "The value to pass as page[before] to walk backward."
        }
      }
    },
    "links": {
      "type": "object",
      "description": "Present when page[size] was supplied.",
      "properties": {
        "next": {
          "type": ["string", "null"],
          "description": "The full url of the next page, carrying page[after]."
        },
        "prev": {
          "type": ["string", "null"],
          "description": "The full url of the previous page, carrying page[before]."
        }
      }
    },
    "count": {
      "type": "integer",
      "description": "Offset pagination only."
    },
    "next_page": {
      "type": ["string", "null"],
      "description": "Offset pagination only."
    },
    "previous_page": {
      "type": ["string", "null"],
      "description": "Offset pagination only."
    }
  },
  "required": ["tickets"]
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def ticket_list(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]

    r = requests.get(
        f"{api}/tickets",
        headers=auth,
        params=_query(args, ("external_id", "include", "page[size]", "page[after]",
                             "page[before]")),
        timeout=10,
    )
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.4 ticket.search

| | |
|---|---|
| **Documentation** | [Export Search Results](https://developer.zendesk.com/api-reference/ticketing/ticket-management/search/#export-search-results) |

```json
{ "func": "ticket.search", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Find tickets by search query, one cursor-paged page per call. Resolves with the page: meta.has_more says whether another page follows and meta.after_cursor is the value to pass as page[after] on the next call, and expires one hour after it is issued. Results are ordered by created_at only, and are drawn from the search index, so a ticket created moments ago may not answer yet. Rejects invalid_request if the query fails validation, which includes a query carrying a type: term — this endpoint takes the object type separately and always searches tickets — an unsupported field or operator, or a cursor that has expired.",
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "Zendesk search query, e.g. \"status:open assignee:me created>2026-01-01 tags:escalated\". Must not contain a type: term."
    },
    "page[size]": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1000,
      "description": "Results per page, up to 1000; 100 is the documented recommendation, and a large page over an account with many archived tickets can time out."
    },
    "page[after]": {
      "type": "string",
      "description": "The meta.after_cursor of the previous page."
    }
  },
  "required": ["query"]
}
```

### 4.4.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.3.2.

### 4.4.3 Integration Request

```
GET /api/v2/search/export?filter[type]=ticket{?query,page[size],page[after] = promise.param.*} → 200
```

### 4.4.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "results": {
      "type": "array",
      "description": "Ticket records, each carrying result_type \"ticket\".",
      "items": {
        "description": "Same as 4.1.4 .ticket"
      }
    },
    "meta": {
      "type": "object",
      "properties": {
        "has_more": {
          "type": "boolean",
          "description": "false on the last page."
        },
        "after_cursor": {
          "type": ["string", "null"],
          "description": "Expires one hour after it is issued."
        }
      }
    },
    "links": {
      "type": "object",
      "properties": {
        "next": {
          "type": ["string", "null"],
          "description": "The full url of the next page, carrying page[after]."
        },
        "prev": {
          "type": ["string", "null"],
          "description": "Always null; backward pagination is not supported."
        }
      }
    }
  },
  "required": ["results"]
}
```

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def ticket_search(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    params = _query(args, ("query", "page[size]", "page[after]"))
    # The exported object type is given in filter[type]; a type: term inside the
    # query string is an error on this endpoint.
    params["filter[type]"] = "ticket"

    r = _check(requests.get(f"{api}/search/export", headers=auth, params=params,
                            timeout=10))
    if r.status_code >= 400:
        # 422 {"error": "invalid", "description": ...} for a malformed query or
        # an expired cursor.
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.5 ticket.update

| | |
|---|---|
| **Documentation** | [Update Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#update-ticket) |

```json
{ "func": "ticket.update", "args": { ... } }
```

### 4.5.1 Promise Param Schema

```json
{
  "description": "Update one ticket: its status, assignee, group, priority, type, tags and custom fields, and append a comment. Resolves on the 200 with the updated ticket. This endpoint takes no idempotency key and the promise's identity cannot be stamped on an update, so a re-delivery re-sends the whole body: the field values converge on the same result, but a comment in the body is appended a second time. Set safe_update with updated_stamp to have a concurrent change rejected as conflict instead of overwritten. tags replaces the ticket's whole tag list rather than adding to it. Rejects not_found if no ticket with this id is visible to the credentials, conflict if safe_update was set and the ticket changed after updated_stamp, and invalid_request if the update fails validation, which includes every update to a closed ticket, an unknown assignee_id, group_id or custom field id, a custom_status_id whose category disagrees with status, and a comment beyond the ticket's ceiling of 5000 comments.",
  "type": "object",
  "properties": {
    "ticket_id": {
      "type": "integer",
      "description": "The ticket's id. Enumerate via ticket.list or ticket.search."
    },
    "comment": {
      "description": "Same as 4.1.1 .comment"
    },
    "status": {
      "type": "string",
      "enum": ["new", "open", "pending", "hold", "solved", "closed"],
      "description": "The state of the ticket. On an account with custom ticket statuses activated this is the status category of custom_status_id. A ticket cannot be moved out of \"closed\"."
    },
    "custom_status_id": {
      "type": "integer",
      "description": "The custom ticket status, on an account with custom ticket statuses activated. Enumerate via GET /api/v2/custom_statuses."
    },
    "priority": {
      "type": "string",
      "enum": ["urgent", "high", "normal", "low"]
    },
    "type": {
      "type": "string",
      "enum": ["problem", "incident", "question", "task"]
    },
    "subject": {
      "type": "string"
    },
    "assignee_id": {
      "type": "integer",
      "description": "The agent the ticket is assigned to."
    },
    "assignee_email": {
      "type": "string",
      "description": "The email address of the agent to assign the ticket to. Alternative to assignee_id."
    },
    "group_id": {
      "type": "integer",
      "description": "The group the ticket is assigned to. Enumerate via GET /api/v2/groups."
    },
    "requester_id": {
      "type": "integer",
      "description": "The user who requested the ticket."
    },
    "organization_id": {
      "type": "integer",
      "description": "The requester's organization. Only an organization the requester belongs to is accepted."
    },
    "tags": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The tags applied to the ticket. This is a set operation: the array replaces the ticket's existing tags. The additive additional_tags and remove_tags properties are honoured only by PUT /api/v2/tickets/update_many."
    },
    "custom_fields": {
      "description": "Same as 4.1.1 .custom_fields"
    },
    "due_at": {
      "type": ["string", "null"],
      "description": "ISO 8601. Only meaningful on a ticket of type \"task\"."
    },
    "problem_id": {
      "type": "integer",
      "description": "For a ticket of type \"incident\", the id of the problem ticket it is linked to."
    },
    "collaborator_ids": {
      "type": "array",
      "items": {
        "type": "integer"
      },
      "description": "The ids of the users CC'd on the ticket. Replaces the existing list."
    },
    "additional_collaborators": {
      "type": "array",
      "description": "Users to add to the existing CCs, as ids, email addresses, or objects with name and email.",
      "items": {}
    },
    "email_ccs": {
      "description": "Same as 4.1.1 .email_ccs"
    },
    "followers": {
      "description": "Same as 4.1.1 .followers"
    },
    "safe_update": {
      "type": "boolean",
      "description": "When true, the update is rejected with conflict if the ticket changed after updated_stamp. Requires updated_stamp."
    },
    "updated_stamp": {
      "type": "string",
      "description": "ISO 8601 updated_at of the ticket revision this update was composed against. Read with safe_update."
    }
  },
  "required": ["ticket_id"]
}
```

### 4.5.2 Promise Value Schema

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
      "enum": ["not_found", "conflict", "invalid_request"]
    },
    "detail": {
      "description": "not_found: = response.body of the 404 as text; conflict: = response.body of the 409 as text; invalid_request: = response.body of the 4xx as text ({error: \"RecordInvalid\", description, details})"
    }
  },
  "required": ["code"]
}
```

### 4.5.3 Integration Request

```
PUT /api/v2/tickets/{promise.param.ticket_id} → 200
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
        "status": {
          "type": "string",
          "description": "= promise.param.status"
        },
        "custom_status_id": {
          "type": "integer",
          "description": "= promise.param.custom_status_id"
        },
        "priority": {
          "type": "string",
          "description": "= promise.param.priority"
        },
        "type": {
          "type": "string",
          "description": "= promise.param.type"
        },
        "subject": {
          "type": "string",
          "description": "= promise.param.subject"
        },
        "assignee_id": {
          "type": "integer",
          "description": "= promise.param.assignee_id"
        },
        "assignee_email": {
          "type": "string",
          "description": "= promise.param.assignee_email"
        },
        "group_id": {
          "type": "integer",
          "description": "= promise.param.group_id"
        },
        "requester_id": {
          "type": "integer",
          "description": "= promise.param.requester_id"
        },
        "organization_id": {
          "type": "integer",
          "description": "= promise.param.organization_id"
        },
        "tags": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "= promise.param.tags"
        },
        "custom_fields": {
          "type": "array",
          "description": "= promise.param.custom_fields"
        },
        "due_at": {
          "type": ["string", "null"],
          "description": "= promise.param.due_at"
        },
        "problem_id": {
          "type": "integer",
          "description": "= promise.param.problem_id"
        },
        "collaborator_ids": {
          "type": "array",
          "items": {
            "type": "integer"
          },
          "description": "= promise.param.collaborator_ids"
        },
        "additional_collaborators": {
          "type": "array",
          "items": {},
          "description": "= promise.param.additional_collaborators"
        },
        "email_ccs": {
          "type": "array",
          "description": "= promise.param.email_ccs"
        },
        "followers": {
          "type": "array",
          "description": "= promise.param.followers"
        },
        "safe_update": {
          "type": "boolean",
          "description": "= promise.param.safe_update"
        },
        "updated_stamp": {
          "type": "string",
          "description": "= promise.param.updated_stamp"
        }
      }
    }
  },
  "required": ["ticket"]
}
```

### 4.5.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "ticket": {
      "description": "Same as 4.1.4 .ticket"
    },
    "audit": {
      "type": "object",
      "description": "The audit record this update generated.",
      "properties": {
        "id": {
          "type": "integer"
        },
        "ticket_id": {
          "type": "integer"
        },
        "created_at": {
          "type": "string",
          "description": "ISO 8601."
        },
        "author_id": {
          "type": "integer"
        },
        "events": {
          "type": "array",
          "description": "One entry per change the update made, including the appended comment.",
          "items": {
            "type": "object",
            "additionalProperties": true
          }
        },
        "via": {
          "type": "object",
          "additionalProperties": true
        }
      }
    }
  },
  "required": ["ticket", "audit"]
}
```

### 4.5.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def ticket_update(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    ticket_id = quote(str(args["ticket_id"]), safe="")
    body = {k: v for k, v in args.items() if k != "ticket_id"}

    # This endpoint takes no idempotency key: a re-delivery re-applies the field
    # values, which converge, but appends the comment in the body a second time.
    r = requests.put(f"{api}/tickets/{ticket_id}", headers=auth,
                     json={"ticket": body}, timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    if r.status_code == 409:
        # safe_update: the ticket changed after updated_stamp.
        return ("rejected", {"code": "conflict", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        # 422 RecordInvalid, which is also the answer for any update to a
        # closed ticket.
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json()["ticket"])
```

### 4.6 ticket.delete

| | |
|---|---|
| **Documentation** | [Delete Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#delete-ticket) |

```json
{ "func": "ticket.delete", "args": { ... } }
```

### 4.6.1 Promise Param Schema

```json
{
  "description": "Delete one ticket. The delete is soft: the ticket leaves GET /api/v2/tickets, is listed by GET /api/v2/deleted_tickets, and can be put back with PUT /api/v2/deleted_tickets/{ticket_id}/restore until it is purged. Resolves on the 204 with the deleted id. This endpoint takes no idempotency key and a deleted ticket is no longer visible to it, so a re-delivery after a successful delete rejects not_found. Rejects not_found if no ticket with this id is visible to the credentials, and invalid_request if the delete fails validation.",
  "type": "object",
  "properties": {
    "ticket_id": {
      "type": "integer",
      "description": "The ticket's id. Enumerate via ticket.list or ticket.search."
    }
  },
  "required": ["ticket_id"]
}
```

### 4.6.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "= promise.param.ticket_id"
    },
    "deleted": {
      "type": "boolean",
      "description": "true"
    }
  }
}
```

#### Rejected

Same as 4.2.2.

### 4.6.3 Integration Request

```
DELETE /api/v2/tickets/{promise.param.ticket_id} → 204
```

### 4.6.4 Integration Response

```json
{
  "type": "null",
  "description": "204 No Content; the response carries no body."
}
```

### 4.6.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def ticket_delete(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    ticket_id = quote(str(args["ticket_id"]), safe="")

    r = requests.delete(f"{api}/tickets/{ticket_id}", headers=auth, timeout=10)
    # Also the answer once this promise's own delete has landed: the ticket is
    # soft-deleted and this endpoint no longer sees it.
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", {"id": args["ticket_id"], "deleted": True})
```

### 4.7 ticket.merge

| | |
|---|---|
| **Documentation** | [Merge Tickets into Target Ticket](https://developer.zendesk.com/api-reference/ticketing/tickets/tickets/#merge-tickets-into-target-ticket) |

```json
{ "func": "ticket.merge", "args": { ... } }
```

### 4.7.1 Promise Param Schema

```json
{
  "description": "Merge the comments of one or more source tickets into a target ticket and close the sources, and observe the background job the merge runs as to a terminal state. Resolves when the job reaches \"completed\", carrying results, one entry per source ticket with its own success flag — a job can complete with a source that was not merged. Duration is seconds to a few minutes, scaling with the number of sources. This endpoint takes no idempotency key, so a re-delivery queues a second merge; by then the sources are closed and the second job reports them as failures. Rejects not_found if the target ticket does not exist, invalid_request if the request fails validation, job_not_found if the job status record disappears while polling, merge_failed on job status \"failed\", and killed on job status \"killed\".",
  "type": "object",
  "properties": {
    "ticket_id": {
      "type": "integer",
      "description": "The target ticket, which receives the merged comments. A solved or closed ticket cannot receive a merge."
    },
    "ids": {
      "type": "array",
      "items": {
        "type": "integer"
      },
      "description": "The source tickets, merged into the target and then closed. A source ticket must be below \"solved\" — \"new\", \"open\" or \"pending\", or a custom status in one of those categories."
    },
    "target_comment": {
      "type": "string",
      "description": "Comment added to the target ticket recording the merge. Attachments of the source tickets are carried in it."
    },
    "source_comment": {
      "type": "string",
      "description": "Comment added to each source ticket recording the merge."
    },
    "target_comment_is_public": {
      "type": "boolean",
      "description": "Whether the target comment is public. Comments default to private, and stay private regardless of this flag when any ticket involved is private or was created through X, Facebook or the Channel framework."
    },
    "source_comment_is_public": {
      "type": "boolean",
      "description": "Whether the source comments are public. Same restrictions as target_comment_is_public."
    }
  },
  "required": ["ticket_id", "ids"]
}
```

### 4.7.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "= response.body.job_status.id"
    },
    "url": {
      "type": "string",
      "description": "= response.body.job_status.url"
    },
    "status": {
      "type": "string",
      "description": "= response.body.job_status.status"
    },
    "message": {
      "type": ["string", "null"],
      "description": "= response.body.job_status.message"
    },
    "progress": {
      "type": ["integer", "null"],
      "description": "= response.body.job_status.progress"
    },
    "total": {
      "type": ["integer", "null"],
      "description": "= response.body.job_status.total"
    },
    "results": {
      "description": "= response.body.job_status.results"
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
      "enum": ["not_found", "invalid_request", "job_not_found", "merge_failed", "killed"]
    },
    "detail": {
      "description": "not_found / invalid_request: = response.body of the 4xx as text; job_not_found: = response.body of the 404 as text; merge_failed / killed: = response.body.job_status (the terminal job status object)"
    }
  },
  "required": ["code"]
}
```

### 4.7.3 Integration Request

```
POST /api/v2/tickets/{promise.param.ticket_id}/merge → 200
```

```json
{
  "type": "object",
  "properties": {
    "ids": {
      "type": "array",
      "items": {
        "type": "integer"
      },
      "description": "= promise.param.ids"
    },
    "target_comment": {
      "type": "string",
      "description": "= promise.param.target_comment"
    },
    "source_comment": {
      "type": "string",
      "description": "= promise.param.source_comment"
    },
    "target_comment_is_public": {
      "type": "boolean",
      "description": "= promise.param.target_comment_is_public"
    },
    "source_comment_is_public": {
      "type": "boolean",
      "description": "= promise.param.source_comment_is_public"
    }
  },
  "required": ["ids"]
}
```

### 4.7.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "job_status": {
      "type": "object",
      "properties": {
        "id": {
          "type": "string",
          "description": "Assigned when the job is queued."
        },
        "url": {
          "type": "string",
          "description": "The url to poll for status updates."
        },
        "job_type": {
          "type": "string",
          "description": "The type of the job."
        },
        "status": {
          "type": "string",
          "enum": ["queued", "working", "completed", "failed", "killed"],
          "description": "\"queued\" and \"working\" are in flight. \"completed\", \"failed\" and \"killed\" are terminal. A job that reports \"completed\" may still carry unsuccessful entries in results."
        },
        "message": {
          "type": ["string", "null"],
          "description": "Message from the job worker."
        },
        "progress": {
          "type": ["integer", "null"],
          "description": "Number of tasks already completed."
        },
        "total": {
          "type": ["integer", "null"],
          "description": "Number of tasks this job batches through."
        },
        "results": {
          "description": "One entry per task once the job has run, or a single object carrying success for a job with one task.",
          "oneOf": [
            {
              "type": "array",
              "items": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                  "id": {
                    "type": "integer",
                    "description": "The id of the resource the task acted on."
                  },
                  "action": {
                    "type": "string",
                    "description": "The action the task attempted, e.g. \"update\"."
                  },
                  "status": {
                    "type": "string",
                    "description": "The outcome, e.g. \"Updated\"."
                  },
                  "success": {
                    "type": "boolean",
                    "description": "Whether this task succeeded."
                  },
                  "error": {
                    "type": "string",
                    "description": "Present on an unsuccessful entry."
                  },
                  "details": {
                    "type": "string",
                    "description": "Present on an unsuccessful entry."
                  }
                }
              }
            },
            {
              "type": "object",
              "properties": {
                "success": {
                  "type": "boolean"
                }
              }
            },
            {
              "type": "null"
            }
          ]
        }
      },
      "required": ["id", "url", "status"]
    }
  },
  "required": ["job_status"]
}
```

### 4.7.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_poll` |

```python
def ticket_merge(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    ticket_id = quote(str(args["ticket_id"]), safe="")
    body = {k: v for k, v in args.items() if k != "ticket_id"}

    # No idempotency key on this endpoint, and nothing on the job status ties it
    # to the promise: a re-delivery queues a second merge whose sources are by
    # then closed, so its results report them as failures.
    r = requests.post(f"{api}/tickets/{ticket_id}/merge", headers=auth,
                      json=body, timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})

    job = r.json()["job_status"]
    failures = 0
    while job["status"] not in JOB_TERMINAL:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(min(cfg.poll.total_seconds(),
                       (promise.timeout_at - time.time() * 1000) / 1000))
        job_id = quote(str(job["id"]), safe="")
        try:
            r = requests.get(f"{api}/job_statuses/{job_id}", headers=auth,
                             timeout=10)
            if r.status_code == 404:
                return ("rejected", {"code": "job_not_found", "detail": r.text})
            _check(r)
            if r.status_code >= 400:
                return ("rejected", {"code": "invalid_request", "detail": r.text})
            job = r.json()["job_status"]
            failures = 0
        except Exception as e:
            if e.args[:1] == ("halt",):
                raise
            failures += 1
            if failures >= 5:
                raise

    if job["status"] == "completed":
        keys = ("id", "url", "status", "message", "progress", "total", "results")
        return ("resolved", {k: job.get(k) for k in keys})
    if job["status"] == "killed":
        return ("rejected", {"code": "killed", "detail": job})
    return ("rejected", {"code": "merge_failed", "detail": job})
```

### 4.8 ticketcomment.list

| | |
|---|---|
| **Documentation** | [List Comments](https://developer.zendesk.com/api-reference/ticketing/tickets/ticket_comments/#list-comments) |

```json
{ "func": "ticketcomment.list", "args": { ... } }
```

### 4.8.1 Promise Param Schema

```json
{
  "description": "List one page of a ticket's comments — the ticket's conversation, which the ticket record itself does not carry. Resolves with the page, oldest comment first; with page[size] supplied, meta.has_more says whether another page follows, meta.after_cursor is the value to pass as page[after] on the next call, and meta.before_cursor the value to pass as page[before] to walk back. Rejects not_found if no ticket with this id is visible to the credentials, which is also the answer for a ticket that has been deleted, and invalid_request if the request fails validation (a malformed or expired cursor).",
  "type": "object",
  "properties": {
    "ticket_id": {
      "type": "integer",
      "description": "The ticket's id. Enumerate via ticket.list or ticket.search."
    },
    "include": {
      "type": "string",
      "description": "Accepts \"users\", side-loading the comment authors and email CCs as a top-level users array. A user deleted since the comment was written is represented by the CC'd email address on a comment from email, and by the user name otherwise."
    },
    "include_inline_images": {
      "type": "boolean",
      "description": "When true, inline images are listed among a comment's attachments as well. false when omitted."
    },
    "sort": {
      "type": "string",
      "enum": ["created_at", "-created_at"],
      "description": "Order of the comments under cursor pagination: \"created_at\" ascending, \"-created_at\" descending. Ascending when omitted."
    },
    "page[size]": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100,
      "description": "Comments per page, up to 100. Supplying it selects cursor pagination."
    },
    "page[after]": {
      "type": "string",
      "description": "The meta.after_cursor of the previous page."
    },
    "page[before]": {
      "type": "string",
      "description": "The meta.before_cursor of the previous page, walking backward."
    }
  },
  "required": ["ticket_id"]
}
```

### 4.8.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.2.2.

### 4.8.3 Integration Request

```
GET /api/v2/tickets/{promise.param.ticket_id}/comments{?include,include_inline_images,sort,page[size],page[after],page[before] = promise.param.*} → 200
```

### 4.8.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "comments": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer",
            "description": "Assigned when the comment is created."
          },
          "type": {
            "type": "string",
            "enum": ["Comment", "VoiceComment"],
            "description": "A voice comment carries a recording_url in place of a body written by a person."
          },
          "author_id": {
            "type": "integer"
          },
          "body": {
            "type": "string",
            "description": "The comment as text."
          },
          "html_body": {
            "type": "string",
            "description": "The comment as HTML."
          },
          "plain_body": {
            "type": "string",
            "description": "The comment stripped to plain text."
          },
          "public": {
            "type": "boolean",
            "description": "true for a public comment, false for an internal note."
          },
          "created_at": {
            "type": "string",
            "description": "ISO 8601."
          },
          "audit_id": {
            "type": "integer",
            "description": "The ticket audit record this comment belongs to."
          },
          "attachments": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "id": {
                  "type": "integer"
                },
                "file_name": {
                  "type": "string"
                },
                "content_type": {
                  "type": "string"
                },
                "content_url": {
                  "type": "string",
                  "description": "Download url of the file. It may be hosted outside Zendesk."
                },
                "size": {
                  "type": "integer",
                  "description": "Bytes."
                },
                "inline": {
                  "type": "boolean",
                  "description": "true for an image referenced from the comment body, which is excluded from the attachment list unless include_inline_images was set."
                },
                "deleted": {
                  "type": "boolean"
                },
                "malware_scan_result": {
                  "type": "string",
                  "enum": ["malware_found", "malware_not_found", "failed_to_scan", "not_scanned"]
                },
                "thumbnails": {
                  "type": "array",
                  "items": {
                    "type": "object",
                    "additionalProperties": true
                  }
                }
              }
            }
          },
          "metadata": {
            "type": "object",
            "additionalProperties": true,
            "description": "Client, ip address and location of the author, and the comment's flags."
          },
          "via": {
            "type": "object",
            "properties": {
              "channel": {
                "type": "string",
                "description": "How the comment arrived, e.g. \"web\", \"api\", \"email\", \"rule\"."
              },
              "source": {
                "type": "object",
                "additionalProperties": true
              }
            }
          }
        }
      }
    },
    "meta": {
      "description": "Same as 4.3.4 .meta"
    },
    "links": {
      "description": "Same as 4.3.4 .links"
    },
    "count": {
      "type": "integer",
      "description": "Offset pagination only."
    },
    "next_page": {
      "type": ["string", "null"],
      "description": "Offset pagination only."
    },
    "previous_page": {
      "type": ["string", "null"],
      "description": "Offset pagination only."
    }
  },
  "required": ["comments"]
}
```

### 4.8.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def ticketcomment_list(cfg, promise):
    api = _api(cfg)
    auth = _auth(cfg)
    args = promise.param["args"]
    ticket_id = quote(str(args["ticket_id"]), safe="")

    r = requests.get(
        f"{api}/tickets/{ticket_id}/comments",
        headers=auth,
        params=_query(args, ("include", "include_inline_images", "sort",
                             "page[size]", "page[after]", "page[before]")),
        timeout=10,
    )
    # A deleted ticket answers 404 here, as does a ticket the credentials
    # cannot see.
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```
