---
title: Quark Shift & Vigilant Neon: Latência Ultrabaixa com Rust Axum e WebSockets no Bun
summary: Uma investigação comparativa sobre microsserviços de alto desempenho: redirecionamento em microssegundos com Rust e mensageria pub/sub em tempo real com WebSockets nativos no Bun.
cover: /api/v1/media/44444444-90a4-4000-8000-000000000004/quark-shift.png
tags: Rust, Bun, Sistemas Distribuídos, WebSockets, Performance
book_color: "#4a3328"
status: published
---

Na arquitetura de sistemas distribuídos contemporânea, o tempo de resposta em operações críticas de roteamento e mensageria não é apenas uma métrica de vaidade — ele impacta diretamente a retenção de usuários, a vazão máxima de conexões simultâneas e os custos operacionais com servidores.

Para explorar os limites da latência ultrabaixa e do baixo consumo de recursos, desenvolvi dois projetos complementares em ecossistemas de alta performance:
1. **Quark Shift:** Microsserviço de encurtamento, gestão e redirecionamento de links construído nativamente em **Rust (edição 2024)** com Axum, Tokio e SeaORM.
2. **Vigilant Neon:** Plataforma de mensageria Pub/Sub em tempo real que alavanca os **WebSockets nativos em C++ do runtime Bun** com interface reativa em Svelte 5.

---

## 1. Quark Shift: Redirecionamentos na Escala de Microssegundos com Rust

Em um serviço de redirecionamento HTTP (redirecionamento 301 ou 302), qualquer tempo gasto em garbage collection (GC) ou alocações dinâmicas repetidas degrada a experiência do usuário que clicou em um link.

No Quark Shift, estruturamos a aplicação utilizando o **Axum 0.8** sobre o **Tokio**:

```mermaid
sequenceDiagram
    autonumber
    actor Client as Cliente / Navegador
    participant Axum as Axum Router (Rust / Tokio)
    participant Cache as RwLock<LruCache> (Memória Local)
    participant Postgres as PostgreSQL 14 (SeaORM)

    Client->>Axum: GET /r/{short_code}
    Axum->>Cache: Leitura não bloqueante com RwLock::read()
    alt Link Presente no Cache
        Cache-->>Axum: Target URL (tempo de resposta: ~250 µs)
    else Cache Miss
        Axum->>Postgres: SELECT original_url FROM links WHERE code = ?
        Postgres-->>Axum: Registro do Link
        Axum->>Cache: Escrita protegida no Cache LRU
    end
    Axum-->>Client: HTTP 301 Moved Permanently (Location: {target_url})
```

### Características de Performance em Produção

- **Consumo de Memória em Repouso:** O processo compilado estático consome menos de **18 MB de memória RAM** sob carga contínua.
- **Segurança de Memória Absoluta:** Zero risco de data races graças ao sistema de ownership e empréstimos (*borrow checker*) do Rust.
- **Frontend 3D:** Interface em Vue 3 com efeitos imersivos renderizados via Three.js e Vanta.js operando a 60 FPS estáveis.

---

## 2. Vigilant Neon: Pub/Sub com WebSockets Nativos no Engine C++ do Bun

Enquanto linguagens compiladas como Rust brilham em tarefas de infraestrutura estrita, runtimes modernos como o **Bun** trouxeram uma revolução para o ecossistema JavaScript e TypeScript.

No **Vigilant Neon**, exploramos o fato de que o `Bun.serve` não implementa WebSockets através de camadas de emulação em JS (como `ws` ou `socket.io`), mas sim direto nas entranhas em C++ da engine **JavaScriptCore**:

```mermaid
flowchart TD
    subgraph Produtores ["Produtores de Mensagens"]
        HTTPProducer["Sistemas Externos (HTTP POST /api/publish)"]
        SocketProducer["Clientes Web / Sensores (WS Frame)"]
    end

    subgraph CoreEngine ["Vigilant Neon Core (Bun.serve)"]
        Router["HTTP Request Router"]
        TopicTree["Gerenciador de Tópicos Nativo (C++ uWebSockets Engine)"]
        DrizzleLogger["Persistência Assíncrona (PostgreSQL + Drizzle ORM)"]
        Router --> TopicTree
        Router --> DrizzleLogger
    end

    subgraph Assinantes ["Clientes Inscritos em Tempo Real"]
        Client1["Dashboard Svelte 5 (Cliente 1)"]
        Client2["Monitor Mobile (Cliente 2)"]
        TopicTree -->|"server.publish() na casa dos microssegundos"| Client1
        TopicTree -->|"server.publish() na casa dos microssegundos"| Client2
    end

    HTTPProducer --> Router
    SocketProducer --> Router
```

### O Poder do `server.publish` no Bun

Com o Bun, a difusão de uma mensagem para milhares de sockets conectados é executada diretamente em código nativo de máquina:

```typescript
Bun.serve({
  port: 8080,
  websocket: {
    message(ws, message) {
      // Recebe frame e faz broadcast instantâneo no tópico sem passar pelo JS loop
      server.publish(ws.data.topic, message);
    },
    open(ws) {
      ws.subscribe(ws.data.topic);
    },
    close(ws) {
      ws.unsubscribe(ws.data.topic);
    }
  }
});
```

A interface do Vigilant Neon, desenvolvida em **Svelte 5** com o **Carbon Design System**, oferece visualização em tempo real de mensagens trafegadas, taxas de vazão e logs de eventos:

![Painel de Mensageria em Tempo Real do Vigilant Neon](/api/v1/media/55555555-be04-4000-8000-000000000005/vigilant-neon.png)

---

## 3. Matriz Comparativa: Rust vs. Bun

A escolha entre essas tecnologias não é ideológica; é orientada pelo perfil de carga e pelos requisitos da aplicação:

| Critério | Rust (Axum + Tokio) | Bun (Native WebSockets) |
| :--- | :--- | :--- |
| **Consumo de Memória (RAM)** | Ultrabaixo (< 20 MB) | Baixo (~45 - 70 MB) |
| **Tempo de Inicialização (Cold Start)** | Instantâneo (< 5 ms) | Quase instantâneo (< 25 ms) |
| **Velocidade de Desenvolvimento** | Moderada (exige modelagem de tipos e lifetimes) | Muito rápida (TypeScript nativo sem transpilador) |
| **Throughput de WebSockets** | Excelente | Excepcional (implementação nativa em C++) |
| **Segurança em Compilação** | Imbatível (Memory Safety comprovada) | Alta (Tipagem estrita TypeScript) |

---

## 4. Conclusão

Dominar tanto **Rust** quanto **Bun** amplia dramaticamente o leque de soluções de um engenheiro de software: enquanto o primeiro oferece controle cirúrgico sobre a máquina física, o segundo traz produtividade imbatível sem abrir mão de métricas de latência que rivalizam com linguagens de baixo nível.
