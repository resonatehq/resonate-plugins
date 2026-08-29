# n8n

| | |
|---|---|
| **API** | `{base_url}/api/v1` |
| **Idempotency** | No idempotency: no write in the Public API accepts a client-supplied key or id — `POST /api/v1/workflows` assigns the workflow id, and `POST /api/v1/executions/{id}/retry` carries no body field but `loadWorkflow` — so `sanitize(promise.id)` is never injected and no charset or length constraint on it applies. A retry started by an earlier delivery is instead correlated by the source execution id it stamps into `retryOf`, which `GET /api/v1/executions` returns |
| **Reviewed by** | Claude Opus 5, 2026-08-29 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `{base_url}/api/v1/openapi.yml` |
| **Self-hosted** | yes — §5 |

## 1. Address

```
n8n://[{instance}]               # omitted instance = "default"
```

## 2. Configuration

```toml
[n8n.{instance}]                 # [n8n] = [n8n.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://n8n.acme.com` |
| `api_key` | `String` | | `…` |
| `poll` | `Duration` | `5s` | `5s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [ApiKeyAuth]({base_url}/api/v1/openapi.yml#/components/securitySchemes/ApiKeyAuth) |
| **Probe** | `GET /api/v1/workflows?limit=1` → `200` |

```
X-N8N-API-KEY: {api_key}
```

## 4. Operations

### 4.1 workflow.create

| | |
|---|---|
| **Documentation** | [Create a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows/post) |

```json
{ "func": "workflow.create", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Create a workflow. The workflow is created unpublished and its trigger nodes are not registered until workflow.publish runs. Resolves with the created workflow. Rejects not_found when projectId or parentFolderId names something that does not exist, invalid_request when the body is malformed — an unknown property, a read-only property, a missing required property, or a settings.redactionPolicy weaker than the instance's redaction floor. n8n assigns the workflow id and accepts no client-supplied one, so a re-delivery after a lost response creates a second, unpublished workflow.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "name": {
      "type": "string",
      "description": "Workflow name."
    },
    "nodes": {
      "type": "array",
      "description": "The workflow's nodes. An empty array is accepted; publishing needs at least one trigger, webhook or polling node.",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "id": {
            "type": "string",
            "description": "Node id, unique within the workflow."
          },
          "name": {
            "type": "string",
            "description": "Node name, unique within the workflow; connections reference nodes by this name."
          },
          "type": {
            "type": "string",
            "description": "Node type, e.g. \"n8n-nodes-base.webhook\"."
          },
          "typeVersion": {
            "type": "number"
          },
          "position": {
            "type": "array",
            "items": {
              "type": "number"
            },
            "description": "Canvas coordinates, [x, y]."
          },
          "parameters": {
            "type": "object",
            "additionalProperties": true,
            "description": "Node parameters, keyed by the node type's own parameter names."
          },
          "credentials": {
            "type": "object",
            "additionalProperties": true,
            "description": "Credentials the node uses, keyed by credential type name; each value is {id, name}."
          },
          "webhookId": {
            "type": "string"
          },
          "disabled": {
            "type": "boolean"
          },
          "notes": {
            "type": "string"
          },
          "notesInFlow": {
            "type": "boolean"
          },
          "executeOnce": {
            "type": "boolean"
          },
          "alwaysOutputData": {
            "type": "boolean"
          },
          "retryOnFail": {
            "type": "boolean"
          },
          "maxTries": {
            "type": "number"
          },
          "waitBetweenTries": {
            "type": "number"
          },
          "continueOnFail": {
            "type": "boolean",
            "description": "Deprecated in favour of onError; still accepted."
          },
          "onError": {
            "type": "string",
            "description": "What the workflow does when this node errors, e.g. \"stopWorkflow\"."
          },
          "customTelemetryTags": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "tag": {
                "type": "array",
                "items": {
                  "type": "object",
                  "additionalProperties": false,
                  "required": ["key", "value"],
                  "properties": {
                    "key": {
                      "type": "string"
                    },
                    "value": {
                      "type": "string"
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "connections": {
      "type": "object",
      "additionalProperties": true,
      "description": "Outgoing connections keyed by source node name, e.g. {\"Webhook\": {\"main\": [[{\"node\": \"Call\", \"type\": \"main\", \"index\": 0}]]}}."
    },
    "settings": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "saveExecutionProgress": {
          "type": "boolean"
        },
        "saveManualExecutions": {
          "type": "boolean"
        },
        "saveDataErrorExecution": {
          "type": "string",
          "enum": ["all", "none"]
        },
        "saveDataSuccessExecution": {
          "type": "string",
          "enum": ["all", "none"]
        },
        "executionTimeout": {
          "type": "number",
          "description": "Seconds after which a run of this workflow is stopped."
        },
        "errorWorkflow": {
          "type": "string",
          "description": "Id of the workflow holding the error trigger node."
        },
        "timezone": {
          "type": "string",
          "description": "IANA timezone name, e.g. \"America/New_York\"."
        },
        "executionOrder": {
          "type": "string",
          "description": "Node execution order, e.g. \"v1\"."
        },
        "binaryMode": {
          "type": "string",
          "enum": ["separate", "combined"],
          "description": "Derived setting: a value sent on create or update is ignored."
        },
        "callerPolicy": {
          "type": "string",
          "enum": ["any", "none", "workflowsFromAList", "workflowsFromSameOwner"],
          "description": "Which workflows may call this one with the Execute Workflow node. workflowsFromSameOwner when omitted."
        },
        "callerIds": {
          "type": "string",
          "description": "Comma-separated workflow ids, used only with callerPolicy \"workflowsFromAList\"."
        },
        "timeSavedMode": {
          "type": "string",
          "enum": ["fixed", "dynamic"]
        },
        "timeSavedPerExecution": {
          "type": "number",
          "description": "Minutes saved per execution, used only with timeSavedMode \"fixed\"."
        },
        "redactionPolicy": {
          "type": "string",
          "enum": ["none", "non-manual", "manual-only", "all"],
          "description": "Which executions of this workflow have their data redacted. A policy weaker than the instance's redaction floor is rejected with 422; omitting it seeds the instance floor."
        },
        "availableInMCP": {
          "type": "boolean"
        },
        "customTelemetryTags": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["key", "value"],
            "properties": {
              "key": {
                "type": "string"
              },
              "value": {
                "type": "string"
              }
            }
          }
        },
        "credentialResolverId": {
          "type": "string",
          "description": "Derived setting: a value sent on create or update is ignored."
        }
      }
    },
    "staticData": {
      "type": ["string", "object", "null"],
      "description": "State the workflow's nodes persist between runs; a JSON string or an object."
    },
    "pinData": {
      "type": ["object", "null"],
      "additionalProperties": true,
      "description": "Pinned sample data keyed by node name."
    },
    "nodeGroups": {
      "type": "array",
      "description": "Visual groupings of nodes shown as frames on the canvas.",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "name", "nodeIds"],
        "properties": {
          "id": {
            "type": "string"
          },
          "name": {
            "type": "string"
          },
          "description": {
            "type": "string",
            "maxLength": 155
          },
          "nodeIds": {
            "type": "array",
            "items": {
              "type": "string"
            }
          }
        }
      }
    },
    "projectId": {
      "type": "string",
      "description": "Project to create the workflow in. The caller's personal project when omitted."
    },
    "parentFolderId": {
      "type": ["string", "null"],
      "description": "Folder to place the workflow in. The project root when omitted or null."
    }
  },
  "required": ["name", "nodes", "connections", "settings"]
}
```

### 4.1.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx, which names the missing project or folder for not_found and the offending property for invalid_request"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/v1/workflows → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "name": {
      "type": "string",
      "description": "= promise.param.name"
    },
    "nodes": {
      "type": "array",
      "description": "= promise.param.nodes",
      "items": {
        "description": "Same as 4.1.1 .nodes[]"
      }
    },
    "connections": {
      "type": "object",
      "description": "= promise.param.connections"
    },
    "settings": {
      "type": "object",
      "description": "= promise.param.settings"
    },
    "staticData": {
      "type": ["string", "object", "null"],
      "description": "= promise.param.staticData"
    },
    "pinData": {
      "type": ["object", "null"],
      "description": "= promise.param.pinData"
    },
    "nodeGroups": {
      "type": "array",
      "description": "= promise.param.nodeGroups"
    },
    "projectId": {
      "type": "string",
      "description": "= promise.param.projectId"
    },
    "parentFolderId": {
      "type": ["string", "null"],
      "description": "= promise.param.parentFolderId"
    }
  },
  "required": ["name", "nodes", "connections", "settings"]
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "description": "The workflow. tags, shared, activeVersion and parentFolder are present only on the responses that carry them; the operations that reuse this schema each leave some of them out.",
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id, assigned by n8n."
    },
    "name": {
      "type": "string"
    },
    "description": {
      "type": ["string", "null"]
    },
    "active": {
      "type": "boolean",
      "description": "true while a version of the workflow is published."
    },
    "activeVersionId": {
      "type": ["string", "null"],
      "description": "versionId of the published version; null when the workflow is not published."
    },
    "versionId": {
      "type": "string",
      "description": "Identifier of the workflow's latest saved version; advances on every update."
    },
    "versionCounter": {
      "type": "number",
      "description": "Number of saved versions."
    },
    "isArchived": {
      "type": "boolean"
    },
    "sourceWorkflowId": {
      "type": ["string", "null"]
    },
    "triggerCount": {
      "type": "number",
      "description": "Number of trigger nodes in the workflow."
    },
    "nodes": {
      "type": "array",
      "description": "The stored nodes, each with the node type's defaults filled in.",
      "items": {
        "description": "Same as 4.1.1 .nodes[]"
      }
    },
    "connections": {
      "type": "object",
      "additionalProperties": true
    },
    "nodeGroups": {
      "type": "array"
    },
    "settings": {
      "type": ["object", "null"],
      "additionalProperties": true
    },
    "staticData": {
      "type": ["string", "object", "null"]
    },
    "pinData": {
      "type": ["object", "null"]
    },
    "meta": {
      "type": ["object", "null"],
      "description": "Template and onboarding metadata."
    },
    "tags": {
      "type": "array",
      "description": "Tags on the workflow.",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "string"
          },
          "name": {
            "type": "string"
          },
          "createdAt": {
            "type": "string",
            "format": "date-time"
          },
          "updatedAt": {
            "type": "string",
            "format": "date-time"
          }
        }
      }
    },
    "shared": {
      "type": "array",
      "description": "Project shares of the workflow.",
      "items": {
        "type": "object",
        "properties": {
          "role": {
            "type": "string"
          },
          "workflowId": {
            "type": "string"
          },
          "projectId": {
            "type": "string"
          },
          "createdAt": {
            "type": "string",
            "format": "date-time"
          },
          "updatedAt": {
            "type": "string",
            "format": "date-time"
          }
        }
      }
    },
    "activeVersion": {
      "type": ["object", "null"],
      "description": "The published version's stored definition; null when the workflow is not published.",
      "properties": {
        "versionId": {
          "type": "string"
        },
        "workflowId": {
          "type": "string"
        },
        "nodes": {
          "type": "array"
        },
        "connections": {
          "type": "object",
          "additionalProperties": true
        },
        "nodeGroups": {
          "type": "array"
        },
        "authors": {
          "type": "string"
        },
        "name": {
          "type": ["string", "null"]
        },
        "description": {
          "type": ["string", "null"]
        },
        "autosaved": {
          "type": "boolean"
        },
        "createdAt": {
          "type": "string",
          "format": "date-time"
        },
        "updatedAt": {
          "type": "string",
          "format": "date-time"
        }
      }
    },
    "parentFolder": {
      "type": ["object", "null"],
      "description": "The folder the workflow sits in."
    },
    "createdAt": {
      "type": "string",
      "format": "date-time"
    },
    "updatedAt": {
      "type": "string",
      "format": "date-time"
    },
    "message": {
      "type": "string",
      "description": "Error text; present on a 4xx response instead of the workflow."
    }
  }
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
import time
from urllib.parse import quote

import requests

API = "/api/v1"


def _check(r):
    # 401 is a missing or rejected API key; 403 is a key whose scopes do not
    # cover the call. Both end only when an operator issues or re-scopes a key.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    return {"X-N8N-API-KEY": cfg.api_key, "Accept": "application/json"}


def _query(args, *names):
    # n8n reads query values as text: active and excludePinnedData are the
    # strings "true"/"false", and the booleans of /executions are parsed from
    # the same spelling, which is not Python's True/False.
    params = {}
    for n in names:
        if n in args:
            v = args[n]
            params[n] = str(v).lower() if isinstance(v, bool) else v
    return params


def workflow_create(cfg, promise):
    args = promise.param["args"]

    # n8n assigns the id; a duplicate from a re-delivery is a second
    # unpublished workflow, which runs nothing until it is published.
    r = _check(
        requests.post(
            f"{cfg.base_url}{API}/workflows",
            headers=_auth(cfg) | {"Content-Type": "application/json"},
            json=args,
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.2 workflow.get

| | |
|---|---|
| **Documentation** | [Retrieve a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{workflowId}/get) |

```json
{ "func": "workflow.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one workflow, including the nodes, connections and settings that constrain workflow.update's body — that body carries only writable properties, so the read-only ones in this response (id, active, activeVersionId, versionId, versionCounter, isArchived, sourceWorkflowId, triggerCount, meta, tags, shared, activeVersion, createdAt, updatedAt) are dropped before it is sent back. Rejects not_found when the workflow id is unknown or the API key's project does not reach it, invalid_request on a malformed request.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id. Enumerate via workflow.list."
    },
    "excludePinnedData": {
      "type": "string",
      "enum": ["true", "false"],
      "description": "\"true\" omits pinData from the response. \"false\" when omitted."
    }
  },
  "required": ["id"]
}
```

### 4.2.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/v1/workflows/{promise.param.id}{?excludePinnedData = promise.param.*} → 200
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def workflow_get(cfg, promise):
    args = promise.param["args"]
    workflow_id = quote(args["id"], safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/workflows/{workflow_id}",
            headers=_auth(cfg),
            params=_query(args, "excludePinnedData"),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.3 workflow.list

| | |
|---|---|
| **Documentation** | [Retrieve all workflows]({base_url}/api/v1/openapi.yml#/paths/~1workflows/get) |

```json
{ "func": "workflow.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "List workflows, one page per promise: the response carries nextCursor, and paging through it is the caller's loop. Resolves with the page. Rejects invalid_request on a malformed request — a cursor n8n did not issue, or a filter outside its allowed values.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "active": {
      "type": "string",
      "enum": ["true", "false"],
      "description": "Keep only published (\"true\") or unpublished (\"false\") workflows."
    },
    "tags": {
      "type": "string",
      "description": "Comma-separated tag names; keeps workflows carrying all of them."
    },
    "name": {
      "type": "string",
      "description": "Keeps workflows whose name matches."
    },
    "projectId": {
      "type": "string",
      "description": "Keeps workflows in one project."
    },
    "excludePinnedData": {
      "type": "string",
      "enum": ["true", "false"],
      "description": "\"true\" omits pinData from every item. \"false\" when omitted."
    },
    "offset": {
      "type": "number",
      "minimum": 0,
      "description": "Items to skip. 0 when omitted."
    },
    "limit": {
      "type": "number",
      "maximum": 250,
      "description": "Items per page. 100 when omitted."
    },
    "cursor": {
      "type": "string",
      "description": "nextCursor of a previous page; the first page when omitted."
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
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/v1/workflows{?active,tags,name,projectId,excludePinnedData,offset,limit,cursor = promise.param.*} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "data": {
      "type": "array",
      "description": "The page's workflows. Each item omits description, versionCounter and sourceWorkflowId.",
      "items": {
        "description": "Same as 4.1.4"
      }
    },
    "nextCursor": {
      "type": ["string", "null"],
      "description": "Cursor of the following page; null on the last page."
    },
    "message": {
      "type": "string",
      "description": "Error text; present on a 4xx response instead of data."
    }
  }
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def workflow_list(cfg, promise):
    args = promise.param["args"]

    # Paging is the caller's loop (one promise per page), not ours.
    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/workflows",
            headers=_auth(cfg),
            params=_query(args, "active", "tags", "name", "projectId",
                          "excludePinnedData", "offset", "limit", "cursor"),
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.4 workflow.update

| | |
|---|---|
| **Documentation** | [Update a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{id}/put) |

```json
{ "func": "workflow.update", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Replace a workflow's definition, saving a new version of it. A published workflow is re-published at the new version unless publishIfActive is false, which saves the version as a draft and leaves the published one serving. Resolves with the stored workflow. Rejects not_found when the workflow id is unknown or the API key's project does not reach it, conflict when the new version cannot be published (an open workflow review, or a webhook path another published workflow already holds) — the version is still saved as a draft — and invalid_request when the body is malformed, which includes sending back the read-only properties of a workflow.get response.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id."
    },
    "name": {
      "type": "string",
      "description": "Workflow name."
    },
    "nodes": {
      "type": "array",
      "items": {
        "description": "Same as 4.1.1 .nodes[]"
      },
      "description": "The workflow's nodes; replaces the stored ones."
    },
    "connections": {
      "type": "object",
      "additionalProperties": true,
      "description": "Same as 4.1.1 .connections"
    },
    "settings": {
      "description": "Same as 4.1.1 .settings"
    },
    "description": {
      "type": "string",
      "description": "Workflow description."
    },
    "staticData": {
      "type": ["string", "object", "null"],
      "description": "Same as 4.1.1 .staticData"
    },
    "pinData": {
      "type": ["object", "null"],
      "additionalProperties": true,
      "description": "Same as 4.1.1 .pinData"
    },
    "nodeGroups": {
      "type": "array",
      "items": {
        "description": "Same as 4.1.1 .nodeGroups[]"
      },
      "description": "Same as 4.1.1 .nodeGroups"
    },
    "parentFolderId": {
      "type": ["string", "null"],
      "description": "Folder to move the workflow into; null moves it to the project root, omitted leaves it where it is."
    },
    "publishIfActive": {
      "type": "boolean",
      "description": "false saves the new version as a draft against the published one instead of releasing it. true when omitted; no effect on an unpublished workflow."
    }
  },
  "required": ["id", "name", "nodes", "connections", "settings"]
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

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found", "invalid_request", "conflict"]
    },
    "detail": {
      "description": "not_found / invalid_request: = response.body.message of the 4xx; conflict: = response.body of the 409, whose reason and workflowReviewRequestId are present when an open workflow review is what blocks publication"
    }
  },
  "required": ["code"]
}
```

### 4.4.3 Integration Request

```
PUT /api/v1/workflows/{promise.param.id}{?publishIfActive = promise.param.*} → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "name": {
      "type": "string",
      "description": "= promise.param.name"
    },
    "nodes": {
      "type": "array",
      "description": "= promise.param.nodes"
    },
    "connections": {
      "type": "object",
      "description": "= promise.param.connections"
    },
    "settings": {
      "type": "object",
      "description": "= promise.param.settings"
    },
    "description": {
      "type": "string",
      "description": "= promise.param.description"
    },
    "staticData": {
      "type": ["string", "object", "null"],
      "description": "= promise.param.staticData"
    },
    "pinData": {
      "type": ["object", "null"],
      "description": "= promise.param.pinData"
    },
    "nodeGroups": {
      "type": "array",
      "description": "= promise.param.nodeGroups"
    },
    "parentFolderId": {
      "type": ["string", "null"],
      "description": "= promise.param.parentFolderId"
    }
  },
  "required": ["name", "nodes", "connections", "settings"]
}
```

### 4.4.4 Integration Response

Same as 4.1.4, plus:

```json
{
  "type": "object",
  "properties": {
    "reason": {
      "type": "string",
      "enum": ["review_pending", "changes_requested"],
      "description": "Present on a 409 whose cause is an open workflow review."
    },
    "workflowReviewRequestId": {
      "type": "string",
      "description": "Present on a 409 whose cause is an open workflow review."
    }
  }
}
```

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def workflow_update(cfg, promise):
    args = promise.param["args"]
    workflow_id = quote(args["id"], safe="")
    body = {k: v for k, v in args.items() if k not in ("id", "publishIfActive")}

    # Re-delivery re-sends the same definition: n8n stores another version of
    # the same content, and the workflow ends in the state this body describes.
    r = _check(
        requests.put(
            f"{cfg.base_url}{API}/workflows/{workflow_id}",
            headers=_auth(cfg) | {"Content-Type": "application/json"},
            params=_query(args, "publishIfActive"),
            json=body,
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code == 409:
        # The version is saved; only its publication is blocked.
        return ("rejected", {"code": "conflict", "detail": r.json()})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.5 workflow.publish

| | |
|---|---|
| **Documentation** | [Publish a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{id}~1publish/post) |

```json
{ "func": "workflow.publish", "args": { ... } }
```

### 4.5.1 Promise Param Schema

```json
{
  "description": "Publish a version of a workflow, registering its triggers and webhooks so production runs start — the action n8n v1 called activating. Publishing an already published workflow succeeds and re-publishes it. Resolves with the published workflow, whose activeVersionId is the version now serving. Rejects not_found when the workflow id or the versionId is unknown, conflict when publication is blocked by an open workflow review or by a webhook path another published workflow already holds, and invalid_request when the version cannot be published — no trigger, webhook or polling node among its nodes.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id."
    },
    "versionId": {
      "type": "string",
      "description": "Version to publish. The workflow's latest version when omitted."
    },
    "name": {
      "type": "string",
      "description": "Name recorded on the published version."
    },
    "description": {
      "type": "string",
      "description": "Description recorded on the published version."
    }
  },
  "required": ["id"]
}
```

### 4.5.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request", "conflict"]
    },
    "detail": {
      "description": "not_found / invalid_request: = response.body.message of the 4xx, which distinguishes an unknown workflow from an unknown version; conflict: = response.body of the 409, whose reason and workflowReviewRequestId are present when an open workflow review is what blocks publication"
    }
  },
  "required": ["code"]
}
```

### 4.5.3 Integration Request

```
POST /api/v1/workflows/{promise.param.id}/publish → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "properties": {
    "versionId": {
      "type": "string",
      "description": "= promise.param.versionId"
    },
    "name": {
      "type": "string",
      "description": "= promise.param.name"
    },
    "description": {
      "type": "string",
      "description": "= promise.param.description"
    }
  }
}
```

### 4.5.4 Integration Response

Same as 4.4.4.

### 4.5.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def workflow_publish(cfg, promise):
    args = promise.param["args"]
    workflow_id = quote(args["id"], safe="")
    body = {k: v for k, v in args.items() if k != "id"}

    # Publishing is idempotent: a re-delivery re-publishes the same version and
    # answers 200 with the same workflow.
    r = _check(
        requests.post(
            f"{cfg.base_url}{API}/workflows/{workflow_id}/publish",
            headers=_auth(cfg) | {"Content-Type": "application/json"},
            json=body,
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code == 409:
        return ("rejected", {"code": "conflict", "detail": r.json()})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.6 workflow.unpublish

| | |
|---|---|
| **Documentation** | [Unpublish a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{id}~1unpublish/post) |

```json
{ "func": "workflow.unpublish", "args": { ... } }
```

### 4.6.1 Promise Param Schema

```json
{
  "description": "Unpublish a workflow, unregistering its triggers and webhooks so no further production run starts — the action n8n v1 called deactivating. Runs already in flight are left alone. Unpublishing a workflow that is not published succeeds and changes nothing. Resolves with the workflow, whose active is false and activeVersionId null. Rejects not_found when the workflow id is unknown or the API key's project does not reach it, invalid_request on a malformed request.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id."
    }
  },
  "required": ["id"]
}
```

### 4.6.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.6.3 Integration Request

```
POST /api/v1/workflows/{promise.param.id}/unpublish → 200
```

### 4.6.4 Integration Response

Same as 4.1.4.

### 4.6.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def workflow_unpublish(cfg, promise):
    workflow_id = quote(promise.param["args"]["id"], safe="")

    # Unpublishing is idempotent: a re-delivery answers 200 with the same
    # already unpublished workflow.
    r = _check(
        requests.post(
            f"{cfg.base_url}{API}/workflows/{workflow_id}/unpublish",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.7 workflow.archive

| | |
|---|---|
| **Documentation** | [Archive a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{id}~1archive/post) |

```json
{ "func": "workflow.archive", "args": { ... } }
```

### 4.7.1 Promise Param Schema

```json
{
  "description": "Archive a workflow — n8n's soft delete: the workflow is unpublished, saved as a new version and kept out of the way without being lost, and workflow.unarchive brings it back. Archiving an already archived workflow succeeds and returns it unchanged, so a re-delivery settles the same way. Resolves with the archived workflow, whose isArchived is true, active false and activeVersionId null. Rejects not_found when the workflow id is unknown or the API key's project does not reach it, invalid_request on a malformed request.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id."
    }
  },
  "required": ["id"]
}
```

### 4.7.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.7.3 Integration Request

```
POST /api/v1/workflows/{promise.param.id}/archive → 200
```

### 4.7.4 Integration Response

Same as 4.1.4.

### 4.7.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def workflow_archive(cfg, promise):
    workflow_id = quote(promise.param["args"]["id"], safe="")

    # Archiving is idempotent: n8n answers 200 with the current workflow when it
    # is already archived, so a re-delivery settles the same way.
    r = _check(
        requests.post(
            f"{cfg.base_url}{API}/workflows/{workflow_id}/archive",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.8 workflow.unarchive

| | |
|---|---|
| **Documentation** | [Unarchive a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{id}~1unarchive/post) |

```json
{ "func": "workflow.unarchive", "args": { ... } }
```

### 4.8.1 Promise Param Schema

```json
{
  "description": "Restore an archived workflow, saving a new version of it. The workflow comes back unpublished whatever it was before it was archived; workflow.publish is what puts it back in service. n8n answers 400 for a workflow that is not archived, which is also what a re-delivery meets once the unarchive has landed, so the workflow is re-read on a 400 and the promise resolves when it is already unarchived. Resolves with the workflow, whose isArchived is false. Rejects not_found when the workflow id is unknown or the API key's project does not reach it, invalid_request on a malformed request.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id."
    }
  },
  "required": ["id"]
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

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.8.3 Integration Request

```
POST /api/v1/workflows/{promise.param.id}/unarchive → 200
```

### 4.8.4 Integration Response

Same as 4.1.4.

### 4.8.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def workflow_unarchive(cfg, promise):
    workflow_id = quote(promise.param["args"]["id"], safe="")

    r = _check(
        requests.post(
            f"{cfg.base_url}{API}/workflows/{workflow_id}/unarchive",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code == 400:
        # n8n answers 400 "Workflow is not archived." — the same answer a
        # re-delivery meets once the unarchive landed, so the stored workflow
        # decides: not archived is the state this call asks for.
        g = _check(
            requests.get(
                f"{cfg.base_url}{API}/workflows/{workflow_id}",
                headers=_auth(cfg),
                timeout=10,
            )
        )
        if g.status_code == 200 and not g.json()["isArchived"]:
            return ("resolved", g.json())
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.9 workflow.delete

| | |
|---|---|
| **Documentation** | [Delete a workflow]({base_url}/api/v1/openapi.yml#/paths/~1workflows~1{id}/delete) |

```json
{ "func": "workflow.delete", "args": { ... } }
```

### 4.9.1 Promise Param Schema

```json
{
  "description": "Delete a workflow, published or not, along with its versions and its executions. Resolves with the deleted workflow as it last stood. Rejects not_found when the workflow id is unknown or the API key's project does not reach it — which is also what a re-delivery meets once the delete has gone through — and invalid_request on a malformed request.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Workflow id."
    }
  },
  "required": ["id"]
}
```

### 4.9.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.9.3 Integration Request

```
DELETE /api/v1/workflows/{promise.param.id} → 200
```

### 4.9.4 Integration Response

Same as 4.1.4.

### 4.9.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def workflow_delete(cfg, promise):
    workflow_id = quote(promise.param["args"]["id"], safe="")

    # The delete takes the workflow's executions with it, so a re-delivery
    # after a lost response finds nothing and rejects not_found.
    r = _check(
        requests.delete(
            f"{cfg.base_url}{API}/workflows/{workflow_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.10 execution.list

| | |
|---|---|
| **Documentation** | [Retrieve all executions]({base_url}/api/v1/openapi.yml#/paths/~1executions/get) |

```json
{ "func": "execution.list", "args": { ... } }
```

### 4.10.1 Promise Param Schema

```json
{
  "description": "List execution records — what a published workflow produced, and where the ids execution.retry takes come from. One page per promise: the response carries nextCursor, and paging through it is the caller's loop. Executions that are still running are left out unless status is \"running\". A plain read — not the completion mechanism; execution.retry observes independently. Resolves with the page. Rejects invalid_request on a malformed request — a cursor n8n did not issue, a status outside the enum, or a limit above 250.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "status": {
      "type": "string",
      "enum": ["canceled", "crashed", "error", "new", "running", "success", "unknown", "waiting"],
      "description": "Keeps executions in one state. Without it the page omits running executions."
    },
    "workflowId": {
      "type": "string",
      "description": "Keeps executions of one workflow."
    },
    "projectId": {
      "type": "string",
      "description": "Keeps executions of the workflows in one project."
    },
    "includeData": {
      "type": "boolean",
      "description": "true adds each execution's run data. false when omitted."
    },
    "ignoreDataSizeLimit": {
      "type": "boolean",
      "description": "true returns run data past the instance's display-size limit; without it an oversized execution comes back without its data. false when omitted."
    },
    "redactExecutionData": {
      "type": "boolean",
      "description": "true redacts the run data; false asks for unredacted data and needs the execution:reveal scope; the workflow's own redaction policy applies when omitted."
    },
    "limit": {
      "type": "number",
      "maximum": 250,
      "description": "Items per page. 100 when omitted."
    },
    "cursor": {
      "type": "string",
      "description": "nextCursor of a previous page; the first page when omitted."
    }
  }
}
```

### 4.10.2 Promise Value Schema

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
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.10.3 Integration Request

```
GET /api/v1/executions{?status,workflowId,projectId,includeData,ignoreDataSizeLimit,redactExecutionData,limit,cursor = promise.param.*} → 200
```

### 4.10.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "data": {
      "type": "array",
      "description": "The page's executions, newest id first. Each item carries the summary fields; the run data and the fields recorded with it — data, storedAt, jsonSizeBytes and workflowVersionId — are in an item only when includeData is set.",
      "items": {
        "description": "Same as 4.12.4"
      }
    },
    "nextCursor": {
      "type": ["string", "null"],
      "description": "Cursor of the following page; null on the last page."
    },
    "message": {
      "type": "string",
      "description": "Error text; present on a 4xx response instead of data."
    }
  }
}
```

### 4.10.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_list(cfg, promise):
    args = promise.param["args"]

    # Paging is the caller's loop (one promise per page), not ours.
    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/executions",
            headers=_auth(cfg),
            params=_query(args, "status", "workflowId", "projectId", "includeData",
                          "ignoreDataSizeLimit", "redactExecutionData", "limit",
                          "cursor"),
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.11 execution.get

| | |
|---|---|
| **Documentation** | [Retrieve an execution]({base_url}/api/v1/openapi.yml#/paths/~1executions~1{id}/get) |

```json
{ "func": "execution.get", "args": { ... } }
```

### 4.11.1 Promise Param Schema

```json
{
  "description": "Read one execution — its status, and with includeData the run data execution.list leaves out. The read for an execution id already in hand, which execution.list cannot filter by. A plain read — not the completion mechanism; execution.retry observes independently. Resolves with the execution. Rejects not_found when the execution id is unknown, the record has been deleted, or the API key's project does not reach it, invalid_request on a malformed request — an id that is not a decimal integer.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": ["string", "number"],
      "description": "Execution id, a decimal integer. Enumerate via execution.list."
    },
    "includeData": {
      "type": "boolean",
      "description": "true adds the execution's run data and the workflow definition it ran. false when omitted."
    },
    "ignoreDataSizeLimit": {
      "type": "boolean",
      "description": "true returns run data past the instance's display-size limit; without it an oversized execution comes back without its data. false when omitted."
    },
    "redactExecutionData": {
      "type": "boolean",
      "description": "true redacts the run data; false asks for unredacted data and needs the execution:reveal scope; the workflow's own redaction policy applies when omitted."
    }
  },
  "required": ["id"]
}
```

### 4.11.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.11.3 Integration Request

```
GET /api/v1/executions/{promise.param.id}{?includeData,ignoreDataSizeLimit,redactExecutionData = promise.param.*} → 200
```

### 4.11.4 Integration Response

Same as 4.12.4.

### 4.11.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_get(cfg, promise):
    args = promise.param["args"]
    execution_id = quote(str(args["id"]), safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/executions/{execution_id}",
            headers=_auth(cfg),
            params=_query(args, "includeData", "ignoreDataSizeLimit",
                          "redactExecutionData"),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

### 4.12 execution.retry

| | |
|---|---|
| **Documentation** | [Retry an execution]({base_url}/api/v1/openapi.yml#/paths/~1executions~1{id}~1retry/post) |

```json
{ "func": "execution.retry", "args": { ... } }
```

### 4.12.1 Promise Param Schema

```json
{
  "description": "Run a failed execution again from where it stopped and observe the new execution to a terminal state. Resolves when it reaches \"success\"; rejects execution_failed on \"error\", execution_canceled on \"canceled\", execution_crashed on \"crashed\", not_found when the source execution id is unknown or the API key's project does not reach it, not_retryable when the source execution cannot be retried — it already succeeded, it was aborted before its data was saved, or it is still queued — workflow_changed when loadWorkflow is true and the retry cannot start against the workflow's current definition, invalid_request on a malformed request, and deleted if the new execution is removed before its terminal state is observed. The states \"new\", \"running\", \"waiting\" and \"unknown\" are not terminal and are polled until timeoutAt; \"waiting\" is an execution paused at a Wait node, which resumes at its waitTill and can sit there indefinitely. Duration is the workflow's own runtime — seconds to hours — so size timeoutAt to cover a whole run. n8n offers no client-supplied key on this call: a retry started by an earlier delivery, or from the n8n UI, is found by its retryOf and observed rather than started again.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": ["string", "number"],
      "description": "Id of the execution to retry, a decimal integer. Enumerate via execution.list with status \"error\" or \"crashed\"."
    },
    "loadWorkflow": {
      "type": "boolean",
      "description": "true runs the workflow's current definition instead of the one saved with the execution; the instance answers 500 when that definition no longer holds a node the stopped execution was standing on, which is rejected as workflow_changed rather than retried, every re-delivery sending the same bytes and meeting the same 500. false when omitted."
    }
  },
  "required": ["id"]
}
```

### 4.12.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "= response.body.id"
    },
    "status": {
      "type": "string",
      "description": "= response.body.status"
    },
    "finished": {
      "type": "boolean",
      "description": "= response.body.finished"
    },
    "mode": {
      "type": "string",
      "description": "= response.body.mode"
    },
    "retryOf": {
      "type": ["string", "number", "null"],
      "description": "= response.body.retryOf"
    },
    "retrySuccessId": {
      "type": ["string", "null"],
      "description": "= response.body.retrySuccessId"
    },
    "workflowId": {
      "type": "string",
      "description": "= response.body.workflowId"
    },
    "startedAt": {
      "type": "string",
      "description": "= response.body.startedAt"
    },
    "stoppedAt": {
      "type": ["string", "null"],
      "description": "= response.body.stoppedAt"
    },
    "waitTill": {
      "type": ["string", "null"],
      "description": "= response.body.waitTill"
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
      "enum": [
        "not_found",
        "not_retryable",
        "workflow_changed",
        "invalid_request",
        "execution_failed",
        "execution_canceled",
        "execution_crashed",
        "deleted"
      ]
    },
    "detail": {
      "description": "not_found / not_retryable / invalid_request: = response.body.message of the 4xx; workflow_changed: = response.body.message of the 500, which is \"Internal server error\"; execution_failed / execution_canceled / execution_crashed: = response.body of the terminal execution; deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.12.3 Integration Request

```
POST /api/v1/executions/{promise.param.id}/retry → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "properties": {
    "loadWorkflow": {
      "type": "boolean",
      "description": "= promise.param.loadWorkflow"
    }
  }
}
```

### 4.12.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Execution id, a decimal integer. The OpenAPI document types it as a number; the instance returns a string."
    },
    "status": {
      "type": "string",
      "enum": ["canceled", "crashed", "error", "new", "running", "success", "unknown", "waiting"],
      "description": "success, error, canceled and crashed are terminal. new (queued), running, waiting (paused at a Wait node until waitTill) and unknown are not: n8n counts all four as stoppable."
    },
    "finished": {
      "type": "boolean",
      "description": "true only for an execution that ran to the end of its workflow."
    },
    "mode": {
      "type": "string",
      "enum": ["cli", "error", "integrated", "internal", "manual", "retry", "trigger", "webhook", "evaluation", "chat"],
      "description": "retry for an execution started by this call."
    },
    "retryOf": {
      "type": ["string", "number", "null"],
      "description": "Id of the execution this one retries; null otherwise. A number in the retry response, a string in a /executions listing."
    },
    "retrySuccessId": {
      "type": ["string", "null"],
      "description": "On a source execution, the id of the retry of it that succeeded. Absent from the retry response."
    },
    "workflowId": {
      "type": "string",
      "description": "Id of the workflow that ran."
    },
    "startedAt": {
      "type": "string",
      "format": "date-time"
    },
    "stoppedAt": {
      "type": ["string", "null"],
      "description": "null while the execution is running; absent from the retry response."
    },
    "waitTill": {
      "type": ["string", "null"],
      "format": "date-time",
      "description": "When an execution in status waiting resumes. Absent from the retry response."
    },
    "createdAt": {
      "type": "string",
      "format": "date-time"
    },
    "deletedAt": {
      "type": ["string", "null"],
      "format": "date-time"
    },
    "storedAt": {
      "type": "string"
    },
    "workflowVersionId": {
      "type": ["string", "null"]
    },
    "usedPrivateCredentials": {
      "type": "boolean"
    },
    "jsonSizeBytes": {
      "type": "number"
    },
    "binaryDataSizeBytes": {
      "type": "number"
    },
    "customData": {
      "type": "object",
      "additionalProperties": true
    },
    "data": {
      "type": "object",
      "additionalProperties": true,
      "description": "Run data: the per-node results. In the retry response, and in a read that asked for includeData."
    },
    "workflowData": {
      "type": "object",
      "additionalProperties": true,
      "description": "The workflow definition the execution ran. In the retry response."
    },
    "message": {
      "type": "string",
      "description": "Error text; present on a 4xx response instead of the execution."
    }
  }
}
```

### 4.12.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
def _executions_page(cfg, params):
    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/executions",
            headers=_auth(cfg),
            params=params,
            timeout=10,
        )
    )
    if r.status_code >= 400:
        raise Exception("release", r.text)
    return r.json()


def _find_retry(cfg, workflow_id, source_id):
    # A retry carries retryOf = the source execution id and always takes a
    # higher id than its source. /executions answers newest id first and omits
    # running executions unless status=running is asked for, so both are read.
    for status in ("running", "any"):
        cursor = None
        while True:
            params = {"workflowId": workflow_id, "limit": 250}
            if status != "any":
                params["status"] = status
            if cursor is not None:
                params["cursor"] = cursor
            page = _executions_page(cfg, params)
            older = False
            for e in page["data"]:
                if str(e.get("retryOf") or "") == str(source_id):
                    return e
                if int(e["id"]) < int(source_id):
                    older = True
                    break
            cursor = page.get("nextCursor")
            if older or cursor is None:
                break
    return None


def execution_retry(cfg, promise):
    args = promise.param["args"]
    source_id = quote(str(args["id"]), safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/executions/{source_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    workflow_id = r.json()["workflowId"]

    e = _find_retry(cfg, workflow_id, args["id"])
    if e is None:
        body = {k: v for k, v in args.items() if k == "loadWorkflow"}
        try:
            # The call runs the workflow and answers only once the run ends, so
            # it outlives the response timeout on all but the shortest runs; the
            # retry it started keeps running and is picked up by retryOf below.
            p = requests.post(
                f"{cfg.base_url}{API}/executions/{source_id}/retry",
                headers=_auth(cfg) | {"Content-Type": "application/json"},
                json=body,
                timeout=10,
            )
        except requests.Timeout:
            p = None
        if p is not None:
            if p.status_code == 500 and body.get("loadWorkflow"):
                # The current definition no longer holds a node the stopped
                # execution was standing on. n8n cannot start the retry, and a
                # re-delivery sends the same bytes and meets the same 500.
                return ("rejected", {"code": "workflow_changed", "detail": p.json()["message"]})
            _check(p)
            if p.status_code == 404:
                return ("rejected", {"code": "not_found", "detail": p.json()["message"]})
            if p.status_code == 409:
                # The source execution succeeded, was aborted before its data
                # was saved, or is still queued.
                return ("rejected", {"code": "not_retryable", "detail": p.json()["message"]})
            if p.status_code >= 400:
                return ("rejected", {"code": "invalid_request", "detail": p.json()["message"]})
            e = p.json()

    failures = 0
    while True:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        if e is not None and e["status"] in ("success", "error", "canceled", "crashed"):
            break
        time.sleep(
            min(
                cfg.poll.total_seconds(),
                (promise.timeout_at - time.time() * 1000) / 1000,
            )
        )
        try:
            if e is None:
                e = _find_retry(cfg, workflow_id, args["id"])
            else:
                retry_id = quote(str(e["id"]), safe="")
                g = _check(
                    requests.get(
                        f"{cfg.base_url}{API}/executions/{retry_id}",
                        headers=_auth(cfg),
                        timeout=10,
                    )
                )
                if g.status_code == 404:
                    return ("rejected", {"code": "deleted"})
                if g.status_code >= 400:
                    return ("rejected", {"code": "invalid_request", "detail": g.json()["message"]})
                e = g.json()
        except Exception as exc:
            if exc.args[:1] == ("halt",):
                raise
            failures += 1
            if failures >= 5:
                raise
            continue
        failures = 0

    keys = (
        "id",
        "status",
        "finished",
        "mode",
        "retryOf",
        "retrySuccessId",
        "workflowId",
        "startedAt",
        "stoppedAt",
        "waitTill",
    )
    if e["status"] == "success":
        return ("resolved", {k: e[k] for k in keys if k in e})  # the 4.12.2 Resolved mapping
    if e["status"] == "error":
        return ("rejected", {"code": "execution_failed", "detail": e})
    if e["status"] == "canceled":
        return ("rejected", {"code": "execution_canceled", "detail": e})
    return ("rejected", {"code": "execution_crashed", "detail": e})
```

### 4.13 execution.stop

| | |
|---|---|
| **Documentation** | [Stop an execution]({base_url}/api/v1/openapi.yml#/paths/~1executions~1{id}~1stop/post) |

```json
{ "func": "execution.stop", "args": { ... } }
```

### 4.13.1 Promise Param Schema

```json
{
  "description": "Stop an execution in flight — the cancel for a run an execution.retry promise is awaiting. Only an execution in status new, running, waiting or unknown can be stopped: the instance answers 500 for any other status, which is also what a re-delivery meets once the stop has landed, so the execution is re-read on a 500 and its own status decides. Resolves with the execution's state after the stop, and with its state as re-read when it is already canceled. Rejects not_found when the execution id is unknown or the API key's project does not reach it, not_stoppable when the execution had already reached success, error or crashed, invalid_request on a malformed request — an id that is not a decimal integer.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": ["string", "number"],
      "description": "Id of the execution to stop, a decimal integer. Enumerate via execution.list with status \"running\", \"waiting\" or \"new\"."
    }
  },
  "required": ["id"]
}
```

### 4.13.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "mode": {
      "type": "string",
      "description": "= response.body.mode"
    },
    "status": {
      "type": "string",
      "description": "= response.body.status"
    },
    "finished": {
      "type": "boolean",
      "description": "= response.body.finished"
    },
    "startedAt": {
      "type": "string",
      "description": "= response.body.startedAt"
    },
    "stoppedAt": {
      "type": ["string", "null"],
      "description": "= response.body.stoppedAt"
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
      "enum": ["not_found", "not_stoppable", "invalid_request"]
    },
    "detail": {
      "description": "not_found / invalid_request: = response.body.message of the 4xx; not_stoppable: = response.body of the execution as re-read, whose status is the terminal state it had already reached"
    }
  },
  "required": ["code"]
}
```

### 4.13.3 Integration Request

```
POST /api/v1/executions/{promise.param.id}/stop → 200
```

### 4.13.4 Integration Response

```json
{
  "type": "object",
  "description": "The execution's mode and state after the stop.",
  "properties": {
    "mode": {
      "type": "string",
      "enum": ["cli", "error", "integrated", "internal", "manual", "retry", "trigger", "webhook", "evaluation", "chat"]
    },
    "status": {
      "type": "string",
      "enum": ["canceled", "crashed", "error", "new", "running", "success", "unknown", "waiting"],
      "description": "canceled for an execution stopped before it finished."
    },
    "finished": {
      "type": "boolean"
    },
    "startedAt": {
      "type": "string",
      "format": "date-time"
    },
    "stoppedAt": {
      "type": ["string", "null"],
      "format": "date-time",
      "description": "Absent for an execution stopped before it started running."
    },
    "message": {
      "type": "string",
      "description": "Error text; present on a 4xx or 5xx response instead of the stop result. \"Internal server error\" on the 500 that answers an execution whose status is not new, running, waiting or unknown."
    }
  }
}
```

### 4.13.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def execution_stop(cfg, promise):
    execution_id = quote(str(promise.param["args"]["id"]), safe="")
    keys = ("mode", "status", "finished", "startedAt", "stoppedAt")

    p = requests.post(
        f"{cfg.base_url}{API}/executions/{execution_id}/stop",
        headers=_auth(cfg),
        timeout=10,
    )
    if p.status_code == 500:
        # n8n answers 500 for an execution whose status is not new, running,
        # waiting or unknown, and the same 500 for an ordinary server fault, so
        # the execution's own status separates them.
        g = _check(
            requests.get(
                f"{cfg.base_url}{API}/executions/{execution_id}",
                headers=_auth(cfg),
                timeout=10,
            )
        )
        if g.status_code == 404:
            return ("rejected", {"code": "not_found", "detail": g.json()["message"]})
        if g.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": g.json()["message"]})
        e = g.json()
        if e["status"] == "canceled":
            # Already stopped — by an earlier delivery of this promise or by
            # someone else; the state this call asks for holds either way.
            return ("resolved", {k: e[k] for k in keys if k in e})  # the 4.13.2 Resolved mapping
        if e["status"] in ("success", "error", "crashed"):
            return ("rejected", {"code": "not_stoppable", "detail": e})
        raise Exception("release", p.text)
    _check(p)
    if p.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": p.json()["message"]})
    if p.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": p.json()["message"]})
    s = p.json()
    return ("resolved", {k: s[k] for k in keys if k in s})  # the 4.13.2 Resolved mapping
```

### 4.14 execution.delete

| | |
|---|---|
| **Documentation** | [Delete an execution]({base_url}/api/v1/openapi.yml#/paths/~1executions~1{id}/delete) |

```json
{ "func": "execution.delete", "args": { ... } }
```

### 4.14.1 Promise Param Schema

```json
{
  "description": "Delete one execution record, its run data with it. Resolves with the deleted execution as it last stood. Rejects not_found when the execution id is unknown or the API key's project does not reach it — which is also what a re-delivery meets once the delete has gone through — not_deletable while the execution is still running, and invalid_request on a malformed request — an id that is not a decimal integer.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": ["string", "number"],
      "description": "Id of the execution to delete, a decimal integer. Enumerate via execution.list."
    }
  },
  "required": ["id"]
}
```

### 4.14.2 Promise Value Schema

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
      "enum": ["not_found", "not_deletable", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.message of the 4xx, which is \"Cannot delete a running execution\" for not_deletable"
    }
  },
  "required": ["code"]
}
```

### 4.14.3 Integration Request

```
DELETE /api/v1/executions/{promise.param.id} → 200
```

### 4.14.4 Integration Response

Same as 4.12.4.

### 4.14.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def execution_delete(cfg, promise):
    execution_id = quote(str(promise.param["args"]["id"]), safe="")

    # The record is hard-deleted, so a re-delivery after a lost response finds
    # nothing and rejects not_found.
    r = _check(
        requests.delete(
            f"{cfg.base_url}{API}/executions/{execution_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code == 400:
        # n8n answers 400 "Cannot delete a running execution", and the same 400
        # for an id it cannot parse, so the execution's own status separates
        # them. Only status running blocks the delete.
        g = _check(
            requests.get(
                f"{cfg.base_url}{API}/executions/{execution_id}",
                headers=_auth(cfg),
                timeout=10,
            )
        )
        if g.status_code == 200 and g.json()["status"] == "running":
            return ("rejected", {"code": "not_deletable", "detail": r.json()["message"]})
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM python:3.13-alpine AS py
RUN pip install --no-cache-dir requests

FROM n8nio/n8n:2.36.8 AS n8n

FROM node:24-alpine
COPY --from=py /usr/local/bin/python3.13 /usr/local/bin/python3.13
COPY --from=py /usr/local/lib/libpython3.13.so.1.0 /usr/local/lib/libpython3.13.so.1.0
COPY --from=py /usr/local/lib/python3.13 /usr/local/lib/python3.13
COPY --from=n8n /usr/local/lib/node_modules/n8n /usr/local/lib/node_modules/n8n
RUN ln -sf /usr/local/bin/python3.13 /usr/local/bin/python3
ENV NODE_PATH=/usr/local/lib/node_modules \
    NODE_ENV=production \
    N8N_LISTEN_ADDRESS=0.0.0.0 \
    N8N_ENCRYPTION_KEY=plugin-n8n-test \
    N8N_USER_FOLDER=/home/node \
    N8N_DIAGNOSTICS_ENABLED=false \
    N8N_VERSION_NOTIFICATIONS_ENABLED=false \
    N8N_SECURE_COOKIE=false
EXPOSE 5678
CMD ["node", "/usr/local/lib/node_modules/n8n/bin/n8n", "start"]
```

### 5.2 Run

```sh
docker rm -f plugin-n8n-test >/dev/null 2>&1 || true
docker build -t plugin-n8n-test spec/
docker run -d --name plugin-n8n-test -p 5678:5678 plugin-n8n-test

N8N_BASE_URL=http://localhost:5678
COOKIES=$(mktemp)

# One key, minted once: n8n answers 500 "There is already an entry with this
# name" to a second key of the same label, so the loop ends the moment it holds
# a key rather than re-minting on every pass.
while :; do
  curl -s -o /dev/null -X POST "$N8N_BASE_URL/rest/owner/setup" \
    -H 'Content-Type: application/json' \
    -d '{"email":"owner@example.com","firstName":"Plugin","lastName":"Test","password":"Passw0rd1"}'
  curl -s -c "$COOKIES" -o /dev/null -X POST "$N8N_BASE_URL/rest/login" \
    -H 'Content-Type: application/json' \
    -d '{"emailOrLdapLoginId":"owner@example.com","password":"Passw0rd1"}'
  N8N_API_KEY=$(curl -s -b "$COOKIES" -X POST "$N8N_BASE_URL/rest/api-keys" \
    -H 'Content-Type: application/json' \
    -d '{"label":"plugin","expiresAt":null,"scopes":["workflow:create","workflow:read","workflow:update","workflow:delete","workflow:list","workflow:activate","workflow:deactivate","execution:list","execution:read","execution:retry","execution:stop","execution:delete"]}' \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["rawApiKey"])' 2>/dev/null)
  [ -n "$N8N_API_KEY" ] && break
  sleep 5
done

# Serviceable = that key answers on the public API.
while :; do
  curl -sf -o /dev/null -H "X-N8N-API-KEY: $N8N_API_KEY" \
    "$N8N_BASE_URL/api/v1/workflows?limit=1" && break
  sleep 5
done

api() { curl -s -H "X-N8N-API-KEY: $N8N_API_KEY" -H 'Content-Type: application/json' "$@"; }
wfid() { python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'; }

# Fixtures: a webhook trigger feeding one HTTP Request node whose target decides
# the outcome. Both fixture runs fail while the helper workflow is unpublished;
# the helper is published afterwards, so a retry of the ok fixture succeeds while
# a retry of the fail fixture (whose target is never registered) fails again.
HELPER_WF=$(api -X POST "$N8N_BASE_URL/api/v1/workflows" -d '{
  "name": "plugin-helper",
  "settings": {"executionOrder": "v1"},
  "nodes": [{"id":"11111111-1111-4111-8111-111111111111","name":"Webhook","type":"n8n-nodes-base.webhook","typeVersion":2,"position":[0,0],"webhookId":"11111111-1111-4111-8111-111111111112","parameters":{"httpMethod":"GET","path":"fixture-helper","options":{}}}],
  "connections": {}}' | wfid)
OK_WF=$(api -X POST "$N8N_BASE_URL/api/v1/workflows" -d '{
  "name": "plugin-ok",
  "settings": {"executionOrder": "v1"},
  "nodes": [{"id":"22222222-2222-4222-8222-222222222221","name":"Webhook","type":"n8n-nodes-base.webhook","typeVersion":2,"position":[0,0],"webhookId":"22222222-2222-4222-8222-222222222222","parameters":{"httpMethod":"POST","path":"fixture-ok","options":{}}},
            {"id":"22222222-2222-4222-8222-222222222223","name":"Call","type":"n8n-nodes-base.httpRequest","typeVersion":4.2,"position":[220,0],"parameters":{"url":"http://localhost:5678/webhook/fixture-helper","options":{}}}],
  "connections": {"Webhook": {"main": [[{"node":"Call","type":"main","index":0}]]}}}' | wfid)
FAIL_WF=$(api -X POST "$N8N_BASE_URL/api/v1/workflows" -d '{
  "name": "plugin-fail",
  "settings": {"executionOrder": "v1"},
  "nodes": [{"id":"33333333-3333-4333-8333-333333333331","name":"Webhook","type":"n8n-nodes-base.webhook","typeVersion":2,"position":[0,0],"webhookId":"33333333-3333-4333-8333-333333333332","parameters":{"httpMethod":"POST","path":"fixture-fail","options":{}}},
            {"id":"33333333-3333-4333-8333-333333333333","name":"Call","type":"n8n-nodes-base.httpRequest","typeVersion":4.2,"position":[220,0],"parameters":{"url":"http://localhost:5678/webhook/fixture-never-registered","options":{}}}],
  "connections": {"Webhook": {"main": [[{"node":"Call","type":"main","index":0}]]}}}' | wfid)

api -o /dev/null -X POST "$N8N_BASE_URL/api/v1/workflows/$OK_WF/publish" -d '{}'
api -o /dev/null -X POST "$N8N_BASE_URL/api/v1/workflows/$FAIL_WF/publish" -d '{}'
curl -s -o /dev/null -X POST "$N8N_BASE_URL/webhook/fixture-ok" \
  -H 'Content-Type: application/json' -d '{}'
curl -s -o /dev/null -X POST "$N8N_BASE_URL/webhook/fixture-fail" \
  -H 'Content-Type: application/json' -d '{}'

failed_execution() {  # $1 = workflow id; prints the id of its failed execution
  while :; do
    ID=$(api "$N8N_BASE_URL/api/v1/executions?workflowId=$1&status=error" \
         | python3 -c 'import json,sys; d=json.load(sys.stdin)["data"]; print(d[0]["id"] if d else "")')
    [ -n "$ID" ] && { echo "$ID"; return; }
    sleep 2
  done
}
N8N_FIXTURE_OK=$(failed_execution "$OK_WF")
N8N_FIXTURE_FAIL=$(failed_execution "$FAIL_WF")
api -o /dev/null -X POST "$N8N_BASE_URL/api/v1/workflows/$HELPER_WF/publish" -d '{}'

export N8N_BASE_URL N8N_API_KEY N8N_FIXTURE_OK N8N_FIXTURE_FAIL
```
