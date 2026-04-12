---
description: "Use when writing or modifying React/TypeScript frontend code in crv-ui. Covers component patterns, styling, and communication with edge daemon."
applyTo: "crv-ui/**"
---
# crv-ui Frontend Conventions

## Tech Stack
- React 19 + TypeScript
- Vite 构建
- Tailwind CSS 4 样式
- React Router DOM 6 路由

## Purpose
为美术等非技术职能提供图形化操作界面，与 crv-edge 守护进程通信，功能对标 P4V。

## Patterns
- 函数式组件 + hooks
- 页面组件放 `src/pages/`
- 公共资源放 `src/assets/` 和 `public/`
