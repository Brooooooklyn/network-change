# `@napi-rs/network-change`

![https://github.com/Brooooooklyn/network-change/actions](https://github.com/Brooooooklyn/network-change/workflows/CI/badge.svg)

**Observe network change event in Node.js.**

> [!IMPORTANT]
> This package is working in progress, and only support Windows and macOS now.

## Install

```
yarn add @napi-rs/network-change
```

```
pnpm add @napi-rs/network-change
```

## Usage

```typescript
import { NwPathMonitor } from '@napi-rs/network-change';


const monitor = new NwPathMonitor();
monitor.start((path) => {
  console.log('network change', path);
});
```
