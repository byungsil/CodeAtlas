# CodeAtlas API Contract

Base URL: `http://localhost:3000`

---

## Data Models

### Symbol

| Field         | Type     | Description                          |
|---------------|----------|--------------------------------------|
| id            | string   | Unique identifier (qualified path)   |
| name          | string   | Short name                           |
| qualifiedName | string   | Fully qualified name with namespaces |
| type          | string   | One of: function, method, class, struct, enum, namespace, variable, typedef |
| filePath      | string   | Relative path from workspace root    |
| line          | number   | Start line (1-based)                 |
| endLine       | number   | End line (1-based)                   |
| signature     | string?  | Full signature (optional)            |
| parentId      | string?  | Parent symbol ID (optional)          |

### Call

| Field    | Type   | Description                |
|----------|--------|----------------------------|
| callerId | string | Caller symbol ID           |
| calleeId | string | Callee symbol ID           |
| filePath | string | File where the call occurs |
| line     | number | Line of the call site      |

### FileRecord

| Field        | Type   | Description                    |
|--------------|--------|--------------------------------|
| path         | string | Relative file path             |
| contentHash  | string | SHA-256 of file content        |
| lastIndexed  | string | ISO 8601 timestamp             |
| symbolCount  | number | Number of symbols in this file |

---

## Endpoints

### GET /function/:name

Retrieve a function or method by name.

**Parameters:**
- `name` (path) — Symbol name to look up. Matches against `name` field.

**Response 200:**

```json
{
  "symbol": { Symbol },
  "callers": [ CallReference ],
  "callees": [ CallReference ]
}
```

**CallReference:**

| Field         | Type   | Description             |
|---------------|--------|-------------------------|
| symbolId      | string | Related symbol ID       |
| symbolName    | string | Short name              |
| qualifiedName | string | Fully qualified name    |
| filePath      | string | File path               |
| line          | number | Line number             |

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

**Notes:**
- If multiple symbols match the name, returns the first match. Use qualified name for precision.

---

### GET /class/:name

Retrieve a class or struct and its members.

**Parameters:**
- `name` (path) — Class name to look up.

**Response 200:**

```json
{
  "symbol": { Symbol },
  "members": [ Symbol ]
}
```

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

---

### GET /search?q=

Search symbols by name substring.

**Parameters:**
- `q` (query) — Search query string. Minimum length: 3 characters.
- `type` (query, optional) — Filter by symbol type.
- `limit` (query, optional) — Max results. Default: 50. Max: 200.

**Response 200:**

```json
{
  "query": "Update",
  "results": [ Symbol ],
  "totalCount": 3,
  "truncated": false
}
```

**Notes:**
- `truncated: true` when `totalCount > limit`.
- Queries shorter than 3 characters return an empty result set.
- Case-insensitive substring match on `name` and `qualifiedName`.

---

### GET /callgraph/:name

Retrieve the call graph rooted at a symbol.

**Parameters:**
- `name` (path) — Root symbol name.
- `depth` (query, optional) — Max traversal depth. Default: 3. Max: 10.

**Response 200:**

```json
{
  "root": {
    "symbol": { id, name, qualifiedName, type, filePath, line },
    "callees": [
      {
        "targetId": "string",
        "targetName": "string",
        "targetQualifiedName": "string",
        "filePath": "string",
        "line": 0
      }
    ]
  },
  "depth": 1,
  "maxDepth": 3,
  "truncated": false
}
```

**Notes:**
- `truncated: true` when the graph was cut short at `maxDepth` and unexplored edges remain.
- Depth 1 returns only direct callees. Depth 2 includes callees-of-callees, etc.

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

---

## Common Behavior

- All responses are `Content-Type: application/json`.
- All file paths are relative to the indexed workspace root.
- Line numbers are 1-based.
- Symbol IDs use the pattern `Namespace::Class::Method` (qualified path).
- Partial results are always explicitly marked with `truncated: true`.

## Error Codes

| Code           | HTTP Status | Description               |
|----------------|-------------|---------------------------|
| NOT_FOUND      | 404         | Symbol not found          |
| BAD_REQUEST    | 400         | Invalid query parameters  |
| INTERNAL_ERROR | 500         | Server-side failure       |
