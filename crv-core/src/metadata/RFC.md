### Branch
```json
// collection: branches
{
  "_id": "branch_main",         // string，内部ID
  "name": "main",               // 对外名称
  "createdAt": 1234567890,
  "createdBy": "userA",
  "headChangelistId": 1000,  // 当前 HEAD 指向的 changelist
  "metadata": {
    "description": "main development branch",
    "owners": ["userA"]
  }
}
```
### Changelist
```json
// collection: changelists
{
  "_id": 1000,             // 递增ID
  "parentChangelistId": [999, 998], // 未来可能有 merge 操作，但是考虑到我们只承认提交了的CL才会有 Changelist ID，所以感觉也没那么必要
  "branchId": "branch_main",
  "author": "userA",
  "description": "fix bug #123",
  "createdAt": 1234567890,
  "updatedAt": 1234567890,

  "committedAt": 12345678910,            // 提交后填入时间

  "filesCount": 1234,             // 统计字段，用于 UI 展示和快速过滤

  "metadata": {
    "labels": ["bugfix", "release-1.2"]
  }
}
```
### File
```json
// collection: files
{
  "_id": "abc123",            // FileID，对路径采用 Blake3 哈希计算得出
  "path": "//src/module/a.cpp",     // 规范化路径
  "createdAt": 1234567890,
  "createdBy": "userA",

  "metadata": {
    "isBinary": false,
    "language": "cpp"
  }
}
```
### FileRevision
```json
// collection: fileRevision
{
  "_id": "abc",      // 对 branch 和 fileId 以及 changelistId 做拼接后使用 blake3 哈希计算得出
  "branchId": "branch_main",
  "fileId": "abc123",             // 隶属于文件 _id
  "changelistId": 1234,    // 来自哪个 changelist

  "binaryId": ["blob_hash_xxx", "blob_hash_xxx"],    // 指向你的二进制存储系统
  "parentRevisionId": ["ab", "a"], // 上一个版本，支持 merge，因此同时持久化两个
  "size": 12340,
  "isDelete": false,              // 标记删除操作的 revision

  "createdAt": 1234567890,         // 或 timestamp
  "metadata": {
    "fileMode": "755",
    "hash": "sha1_xxx"            // 内容 hash，做去重/校验
  }
}
```
### Workspace
```json
// collection: workspaces
{
  "_id": "ws_userA_main", // 工作区全局唯一 ID
  "userId": "userA",

  "baseBranchId": "branch_main",
  "baseChangelistId": 123456,      // workspace 基线

  "overrides": [
    {
      "fileId": "abc123",
      "localRevisionId": "eojdg-dfgjod-fdgjod-dfjgodfjdfjgkj", // 本地未提交 revision / 临时 ID
      "status": "modified"                // modified | added | deleted
    }
  ],

  "createdAt": 1234567890,
  "updatedAt": 1234567890,
  "metadata": {
    "hostname": "dev-machine-01"
  }
}
```