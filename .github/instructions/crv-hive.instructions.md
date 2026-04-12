---
description: "Use when working on crv-hive server, database migrations, Sea-ORM entities, iroh endpoint setup, or PostgreSQL queries."
applyTo: "crv-hive/**"
---
# crv-hive Server Conventions

## Role
中心化版本控制服务器，对标 p4d。监听 `0.0.0.0:34560`。

## Stack
- Sea-ORM + PostgreSQL (数据持久化)
- iroh (与 edge 通信，已移除 gRPC)
- iroh-blobs (大文件 blob 传输)
- tokio async runtime

## Database
- PostgreSQL，通过 Docker Compose 启动：`crv-hive/docker-compose.yml`
- 启动脚本：`crv-hive/scripts/start-db.ps1`（Windows）/ `start-db.sh`（Linux）
- 配置文件：`hive.example.toml`

## iroh Transport
- hive 作为 iroh 端点，不再使用 gRPC
- 活跃实现位于 `crv-hive/src/crv2/` 目录
