# Planejamento: autenticação, usuários e publicação do blog

Data: 2026-09-27. Status: proposta para implementação futura; nenhum endpoint, migration, usuário ou segredo é criado por este documento.

## 1. Objetivo e decisões principais

O blog será público para leitura. A escrita acontecerá em uma área administrativa web autenticada. Inicialmente haverá um proprietário, criado exclusivamente por ferramenta administrativa de seed; o modelo permitirá outros autores futuramente, sem cadastro público.

As decisões abaixo são propostas específicas para este projeto. Limites e prazos são valores iniciais a validar em testes, não garantias de desempenho. Preservar Axum/Tokio, Serde, SQLx e SQLite já existentes e manter cada arquivo de código, teste e documentação abaixo de 500 linhas.

| Tema | Decisão proposta |
| --- | --- |
| Autenticação | JWT de acesso curto; refresh token opaco, rotativo e revogável |
| Navegador | Cookies HttpOnly; frontend e API no mesmo origin, API em `/api/v1` |
| Cadastro | Nenhuma rota de registro, nem mesmo administrativa; provisionamento por CLI |
| Senha | Argon2id com salt aleatório; nunca armazenar senha reversível |
| Autorização | Papéis `owner` e `author`, com verificação de propriedade dos recursos |
| Posts | Markdown como fonte, SQLite como armazenamento canônico, HTML sanitizado derivado |
| Visibilidade | Apenas publicações ativas são públicas; rascunhos e lixeira são privados |
| Busca | SQLite FTS5, sem serviço externo de pesquisa |
| Imagens | Upload autenticado; arquivos fora do Git, do diretório de código e da imagem da aplicação |
| Operação | Uma instância inicial, disco persistente e backup externo verificável |

## 2. Escopo e evolução

Entrega inicial: login/logout, renovação/revogação de sessão, perfil próprio, seed seguro, recuperação administrativa, CRUD editorial completo, tags, imagens, importação/exportação `.md`, listagem pública, busca, filtros e administração web planejada.

Complementos necessários: prevenção de XSS/CSRF, limites de recursos, auditoria, concorrência de edição, lixeira, SEO básico, acessibilidade, estados de erro e política de backup.

Adiar: cadastro público, convites, comentários, OAuth social, newsletter, agendamento, colaboração em tempo real, histórico completo de revisões, múltiplas instâncias e armazenamento S3. Passkeys/MFA são evolução recomendada antes de ampliar o número de autores, não requisito para concluir esta primeira entrega.

## 3. Credenciais e seed do proprietário

O e-mail e a senha comunicados pelo proprietário serão fornecidos somente durante a operação administrativa. Seus valores não aparecem neste documento, exemplos, fixtures, migrations, logs, commits ou configuração de build. Não transformar esta conversa em um arquivo de credenciais.

A senha compartilhada em conversa não deve ser considerada um segredo exclusivamente local. Para produção, gerar uma nova senha longa e exclusiva em um gerenciador no momento do seed. Não executar o seed nem gerar credenciais nesta etapa de planejamento.

### 3.1 Fluxo administrativo proposto

1. Gerar um binário operacional separado, `blog-admin`, que reutiliza serviços de usuários e hashing, sem iniciar HTTP.
2. Executar migrations antes do seed. O schema nunca contém `INSERT` de conta real ou hash de senha predefinido.
3. Executar `blog-admin seed-owner`; solicitar e-mail, nome público e senha em TTY. A senha deve usar entrada sem eco e confirmação.
4. Validar os dados, gerar salt pelo gerador criptográfico do sistema e calcular Argon2id fora da transação de escrita.
5. Em transação curta, verificar que não existe proprietário inicial e inserir a conta com papel `owner` e status `active`.
6. Reexecução com o mesmo e-mail deve informar apenas que o proprietário já existe, sem alterar senha, papel ou perfil. Outro e-mail quando já existe proprietário inicial deve falhar.
7. Proteger corrida entre dois seeds por transação com bloqueio de escrita e restrições únicas. Não depender somente de uma consulta anterior ao INSERT.
8. Exibir apenas resultado e identificador da conta. Descartar buffers sensíveis; nunca imprimir senha, hash ou URL de banco.
9. Fechar o pool e encerrar. O servidor web não dispara seeds em startup, nem cria usuário padrão em caso de banco vazio.

O binário operacional será distribuído como artefato administrativo separado; a imagem HTTP conterá somente o servidor. Executar o seed no host/ambiente operacional com acesso controlado ao mesmo volume SQLite, usando a mesma versão de migrations.

### 3.2 Entrada de segredos e limites reais

Preferir prompt interativo. Em automação futura, aceitar segredo por descritor de arquivo/entrada dedicada de um secret manager ou arquivo temporário montado fora do repositório, com permissões restritas e remoção após o uso. Falhar quando não há fonte explícita; nunca recorrer a senha padrão.

Proibir `--password`, senha em linha de comando, `echo` com senha, comandos salvos no shell, `.env` persistente de seed, Docker `ARG/ENV`, YAML versionado e logs de CI com valores. Arquivos de exemplo descrevem apenas nomes e placeholders. O e-mail também será omitido dos exemplos reais.

A senha em texto existirá brevemente em memória durante seed/login. `secrecy`/`zeroize` reduzem cópias e exposição acidental, mas não prometem proteção contra administrador do host, dumps ou máquina comprometida. O e-mail e o hash precisam existir no banco privado e em seus backups; o segredo original não precisa permanecer no deploy.

Usar `rpassword` para TTY; conferir compatibilidade das versões de `argon2`, `password-hash` e fonte de aleatoriedade. Tipos com senha não devem implementar `Debug` ou `Serialize`. Nenhuma chave JWT será derivada da senha do proprietário.

### 3.3 Administração e recuperação

Planejar subcomandos `user-create`, `user-disable`, `user-reset-password` e `user-set-role`, todos operacionais, auditados e sem endpoints equivalentes de criação. Exigir confirmação do identificador alvo nas operações destrutivas.

Reset de senha gera novo hash e revoga todas as sessões na mesma transação. Recuperação inicial é por acesso administrativo ao host, sem serviço de e-mail. Reexecução do seed não serve como reset. Não permitir desativar ou rebaixar o último proprietário ativo. Contas com posts são desativadas, preservando autoria e histórico.

## 4. Autenticação e ciclo da sessão

### 4.1 JWT de acesso

Proposta: HS256 para uma única API emissora/verificadora, com chave aleatória de pelo menos 32 bytes, carregada por secret manager/arquivo montado. Usar `jsonwebtoken` com backend criptográfico explícito. Avaliar versão estável compatível e RustSec na implementação; não copiar versões de exemplos antigos.

Fixar algoritmo permitido e validar assinatura, `iss`, `aud`, `sub`, `exp`, `iat`, `nbf`, `jti` e `sid`. `sub` identifica usuário; `sid`, sessão; `jti`, emissão. Definir `typ` exclusivo para acesso. Rejeitar algoritmo inesperado, campos obrigatórios ausentes, datas incoerentes, chave desconhecida e tokens maiores que 4 KiB. Tolerância de relógio: até 30 segundos. Estes controles seguem as boas práticas de [JWT RFC 8725](https://www.rfc-editor.org/rfc/rfc8725.html).

TTL inicial de acesso: 10 minutos. Não incluir senha, hash, e-mail ou conteúdo privado no JWT: a assinatura não torna o payload secreto. Papéis no token não serão a autoridade; consultar usuário e sessão ativos no banco a cada operação privada para revogação imediata e atualização de permissões.

`kid` seleciona somente chaves já carregadas em um mapa local, nunca arquivo/URL indicado pelo token. Rotação normal: emitir pela nova chave e aceitar a anterior por 10 minutos mais tolerância; comprometimento: remover imediatamente a chave afetada e revogar sessões. Separar chaves por ambiente e falhar no startup se estiverem ausentes/fracas.

Configuração proposta: `APP_ORIGIN`, `JWT_ISSUER`, `JWT_AUDIENCE`, `JWT_ACTIVE_KEY_ID`, `JWT_KEYS_FILE`, `MEDIA_ROOT` e limites/TTLs validados. O arquivo de chaves fica montado fora da aplicação com permissão mínima; exemplos versionados contêm apenas caminhos fictícios, nunca material criptográfico. Alterar permissões do arquivo/volume e usuário do processo faz parte da preparação operacional.

### 4.2 Cookies e proteção do navegador

Cookies de produção: `__Host-blog_access` e `__Host-blog_refresh`, com `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/` e sem `Domain`. JWT e refresh não vão para localStorage/sessionStorage ou parâmetros de URL. Respostas de auth e área privada usam `Cache-Control: no-store`. A estratégia de cookies segue a [orientação OWASP de sessões](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).

Frontend e API no mesmo origin evitam CORS amplo. Para desenvolvimento, usar proxy local para `/api` e TLS local quando possível; qualquer exceção de cookie sem Secure exige modo development explícito e nomes sem prefixo `__Host-`, proibidos em produção.

Mutações exigem `Origin` exatamente permitido, JSON quando aplicável e `X-CSRF-Token`. Guardar token CSRF aleatório por sessão e compará-lo de forma constante; ele não substitui autenticação. `GET /auth/csrf` devolve o token da sessão validada, com no-store e sem CORS, inclusive usando refresh válido quando o acesso expirou. Não aceitar token CSRF em URL. [Fundamento: OWASP CSRF](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).

Login, que ainda não tem sessão, exige Origin permitido, Content-Type JSON e rejeita origem ausente/`null`; usar Fetch Metadata como defesa adicional. Logout, refresh, upload, preview e todas as outras mutações usam CSRF. GET/HEAD não alteram estado. Cookies SameSite são defesa adicional, não a única proteção.

### 4.3 Login, refresh e logout

1. Login valida tamanho antes do hash, aplica rate limit e busca e-mail normalizado. Conta inexistente realiza verificação com hash fictício válido para reduzir enumeração por tempo.
2. Credencial inválida ou conta inativa responde o mesmo `401 invalid_credentials`; não revelar se o e-mail existe. Sucesso cria sessão, cookies e resposta com perfil seguro.
3. Refresh é um token opaco de 32 bytes aleatórios, base64url. Armazenar somente SHA-256 do token, não o token bruto; essa escolha vale para token aleatório de alta entropia, nunca para senha humana.
4. Prazo absoluto da sessão: 7 dias desde login, sem extensão ilimitada. Cada refresh consome um token e gera outro em transação atômica; o JWT novo não ultrapassa a expiração absoluta da sessão.
5. Registrar tokens consumidos até expirar a família. Reuso de token consumido revoga a sessão/família inteira, inclusive JWTs ainda não expirados. Retornar 401 genérico.
6. O navegador coordena refresh entre abas usando Web Locks/BroadcastChannel; tokens não são transmitidos nesse canal. Sem coordenação, serializar via uma aba responsável ou exigir novo login em conflito. Política estrita: resposta perdida após rotação pode exigir novo login, sem janela de replay.
7. Logout revoga a sessão e limpa ambos os cookies com os mesmos atributos. Logout-all revoga todas as sessões. Aceitar refresh válido para logout quando o JWT expirou, preservando CSRF.
8. Sessões expiradas/revogadas falham em toda rota privada. Limpeza periódica remove históricos somente depois da janela necessária para detectar replay.

### 4.4 Hashing e proteção de recursos

Argon2id: começar com 19 MiB, 2 iterações e paralelismo 1, salt aleatório de pelo menos 16 bytes e formato PHC; calibrar no host real antes do deploy. Essa configuração é um mínimo recomendado pela [OWASP Password Storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html), suportado pela biblioteca [RustCrypto Argon2](https://docs.rs/argon2/latest/argon2/).

Política proposta para novas senhas: 15 a 128 caracteres, limite adicional de 512 bytes, espaços e Unicode aceitos, sem trim/truncamento/normalização silenciosos. Priorizar senha aleatória longa, sem exigência artificial de símbolos. Rehash oportunista após login quando os parâmetros forem atualizados, com controle de versão para não sobrescrever reset concorrente.

Hashing usa `spawn_blocking` com semáforo: inicialmente duas operações simultâneas e fila limitada; rejeitar saturação antes de alocar memória. Timeout de HTTP não interrompe automaticamente trabalho bloqueante, então a permissão só é liberada quando o trabalho terminar.

Login: ponto inicial de 5 tentativas/minuto por conta e 20/minuto por IP, com espera limitada, `429` e `Retry-After`. Não bloquear conta permanentemente por ataques de terceiros. Limitar cardinalidade/TTL dos buckets; refresh, preview, busca e uploads têm orçamentos separados. Confiar em forwarded IP somente do proxy explicitamente configurado.

## 5. Usuários e autorização

Modelo `users`: `id` inteiro, `email`, `normalized_email` UNIQUE, `password_hash`, `display_name`, `bio`, `avatar_media_id` opcional, `role`, `status`, `created_at`, `updated_at`, `password_changed_at`. Datas em UTC, segundos Unix no banco e RFC 3339 nas respostas.

Normalização inicial: trim do e-mail e comparação case-insensitive consistente; aceitar inicialmente endereços ASCII, até 254 bytes. Não remover pontos nem sufixos `+`; não aplicar regras específicas de provedores. Nome público: 1–80 caracteres; bio: até 500. Usar DTO separado para exposição pública, sem e-mail, status administrativo ou dados de sessão.

| Operação | Anônimo | Author | Owner |
| --- | --- | --- | --- |
| Ler posts publicados, tags visíveis e autor público | Sim | Sim | Sim |
| Ler/editar perfil próprio | Não | Sim | Sim |
| Criar/editar/publicar próprios posts | Não | Sim | Sim |
| Administrar posts de outros autores | Não | Não | Sim |
| Listar usuários, desativar, alterar papéis | Não | Não | Sim, mantendo último owner ativo |
| Criar usuário | Não | Não | Somente CLI, fora da API |
| Criar tags para próprios posts | Não | Sim | Sim |
| Renomear/excluir tags globais | Não | Não | Sim |

Consulta privada deve filtrar por usuário autorizado antes de retornar o registro; IDs difíceis de adivinhar não substituem autorização. Usar 404 para recurso inexistente ou não acessível por propriedade e 403 para operação/papel proibido. Autoria é definida no servidor; DTO não aceita `author_id`, `role`, `password_hash` ou status de conta por atribuição genérica.

Alteração de senha própria exige senha atual e revoga todas as sessões, pedindo novo login. Mudança de e-mail fica na CLI nesta fase; evitar um fluxo incompleto de verificação de e-mail. Perfil próprio não muda papel. Usuário inativo não renova sessão nem escreve. Verificar status/posse também na transação de escrita para cobrir revogação concorrente.

## 6. Modelo de dados editorial

| Tabela | Campos e invariantes principais |
| --- | --- |
| `auth_sessions` | `id` aleatório, `user_id` FK, `csrf_token`, `created_at`, `expires_at`, `revoked_at`, metadados mínimos de dispositivo |
| `refresh_tokens` | `token_hash` UNIQUE, `session_id` FK, `created_at`, `expires_at`, `consumed_at`, `replaced_by_hash`; histórico para detectar replay |
| `posts` | `id`, `author_id` FK, `locale`, `slug`, `title`, `description`, `content_markdown`, `content_html`, `search_text`, `renderer_version`, `cover_media_id` FK, `cover_alt`, `status`, `published_at`, `created_at`, `updated_at`, `deleted_at`, `version` |
| `tags` | `id`, `name`, `slug` UNIQUE, datas; nomes legíveis e slug canônico independente do idioma |
| `post_tags` | PK composta (`post_id`, `tag_id`), FKs, sem associação duplicada |
| `media` | `id` aleatório, `owner_id`, `storage_key` UNIQUE, tipo validado, bytes, largura/altura, checksum, estado e datas |
| `post_media` | Relação entre post e mídia usada na capa/corpo, para autorização de leitura e limpeza |
| `audit_events` | Ator opcional, ação, alvo, resultado, request ID e timestamp; sem corpos, tokens ou credenciais |
| `posts_search` | Tabela virtual FTS5 com rowid igual ao post ID e somente conteúdo publicamente visível |

FKs sempre ativas; CHECK para papéis/status/locale/versões positivas; UNIQUE (`locale`, `slug`) incluindo posts na lixeira. Não reutilizar slug enquanto existir registro excluído. Rejeitar conflito com 409; não sobrescrever silenciosamente.

Índices: usuários por e-mail normalizado; sessões por usuário/expiração; refresh por hash/sessão; posts por (`locale`, `published_at`, `id`) com predicado público e por (`author_id`, `status`, `updated_at`, `id`); `post_tags(tag_id, post_id)` e referências de mídia. Ajustar somente com EXPLAIN QUERY PLAN e medições.

`locale` aceita `pt-BR` e `en`; cada post tem um idioma e consultas públicas assumem `pt-BR` quando omitido. Traduções futuras serão posts independentes ligados por grupo explícito; não gerar tradução automática nem misturar versões silenciosamente.

## 7. Posts: conteúdo, validação e ciclo de vida

Campos de criação: `title`, `description`, `content_markdown`, `locale`, `slug` opcional, `cover_media_id`, `cover_alt`, `tag_ids`. Criar sempre como draft; publicar é uma operação explícita.

| Campo | Limite inicial e regra |
| --- | --- |
| `title` | 1–160 caracteres; rejeitar só espaços |
| `description` | Até 320 caracteres; obrigatória e não vazia para publicar |
| `slug` | 3–160 caracteres ASCII minúsculos, números e hífens, sem segmentos de caminho |
| `content_markdown` | Até 256 KiB UTF-8; não vazio para publicar |
| `cover_media_id` | Mídia válida e autorizada; obrigatória para publicar |
| `cover_alt` | 1–200 caracteres para capa; texto acessível obrigatório |
| Tags por post | Até 10 distintas; permitir zero |
| `tag.name` / `tag.slug` | 1–40 caracteres / 2–60 caracteres; slug único |
| PATCH | Campos opcionais tipados; distinguir ausente de `null`; rejeitar campos desconhecidos |

Status: `draft` e `published`; `deleted_at` representa lixeira. Publicar define `published_at` na primeira publicação e valida os requisitos completos. Despublicar volta a draft, remove do índice público e impede leitura pública. Republicar mantém a data da primeira publicação; `updated_at` informa a revisão. Agendamento não é aceito nesta versão.

DELETE faz exclusão lógica e retira imediatamente das consultas/FTS. Restore sempre retorna como draft, exigindo nova publicação. Purga física após 30 dias, via operação administrativa separada; backups seguem retenção própria. Referências públicas antigas podem existir fora do controle do servidor.

Slug é gerado a partir do título só na criação; colisão retorna sugestão ou permite edição antes de salvar. Após a primeira publicação, o slug fica imutável nesta fase, evitando URLs quebradas. Editar título não altera slug. Redirecionamento de slugs pode ser adicionado depois com tabela própria.

Edição de post publicado altera a versão pública imediatamente após salvar; a interface deve avisar isso. Preparação privada de uma revisão publicada exige despublicar ou criar rascunho separado nesta fase. Não prometer staging/revisões sem implementar modelo de versões.

Concorrência: GET privado retorna ETag derivado de `version`; PATCH, DELETE, publish, unpublish e restore exigem `If-Match`. Ausência: 428; versão antiga: 412. Atualização usa `WHERE id = ? AND version = ?`, incrementa versão e executa tags/FTS/auditoria na mesma transação. Não repetir escrita automaticamente após conflito sem recarregar.

## 8. Markdown, importação e renderização

O editor trabalha com Markdown. O banco guarda o texto original como fonte canônica; não manter cópia `.md` sincronizada em diretório de deploy. Importação/exportação oferecem arquivos `.md` sem criar uma segunda fonte de verdade.

Usar [pulldown-cmark](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/) para parse/render e [ammonia](https://docs.rs/ammonia/latest/ammonia/) para sanitização do HTML gerado. Desabilitar HTML bruto no Markdown, MDX/JSX, scripts, iframes, handlers HTML, estilos arbitrários e esquemas perigosos. Sanitização ocorre mesmo com autor confiável.

Permitir títulos, listas, links, blocos de código, citações, tabelas e imagens gerenciadas. Links aceitam HTTPS/HTTP e âncoras; imagens aceitam somente identificadores/URLs de mídia gerenciada. Não buscar imagens ou metadados remotos no servidor; evitar SSRF. Reescrever ou rejeitar atributos fora da allowlist; links externos recebem rel apropriado.

Renderizar/sanitizar no save/preview em trabalho bloqueante limitado, antes de abrir a transação. Armazenar HTML e texto de busca derivados com `renderer_version`; recalcular em lote quando mudar a política. Listagem não renderiza Markdown nem carrega corpos completos. Preview usa exatamente o mesmo pipeline da publicação.

Importação inicial: `.md` UTF-8 puro enviado como `text/markdown` para criar draft; título e demais metadados são editados depois. O importador propõe título a partir do primeiro heading ou usa um título provisório; normaliza o filename apenas para exibição, nunca como caminho de storage. Locale vem de parâmetro validado ou do padrão do blog. Front matter YAML não é executado/interpretado nesta fase; retornar aviso e tratá-lo como texto para não surpreender. Exportação retorna Markdown original com filename seguro e `Content-Disposition: attachment`; apenas para autor/owner.

Frontend só injeta HTML retornado pelo pipeline confiável e aplica CSP como defesa adicional. Preview com erro preserva texto digitado e mostra feedback. Contar palavras/tempo de leitura pelo texto extraído; derivar na escrita, sem prometer estimativa exata.

## 9. Imagens e armazenamento

Planejar `POST /admin/media` multipart autenticado, com CSRF verificado antes de consumir arquivo. Aceitar JPEG, PNG e WebP; rejeitar SVG, HTML e animações inicialmente. Verificar assinatura real, decodificar com limites e reencodar para retirar metadados e conteúdo inesperado. Não confiar em extensão/MIME informado. [Base: OWASP File Upload](https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html).

Limites iniciais: 5 MiB por arquivo, 20 megapixels, no máximo 6000 pixels por dimensão, uma imagem por upload e dois processamentos simultâneos. Tamanho total HTTP multipart até 6 MiB; tempo máximo 30 segundos. Limitar memória do decoder antes de alocar; contar bytes durante streaming mesmo sem Content-Length. Avaliar `image` com codecs estritamente necessários.

Gravar em staging fora da árvore pública com nome aleatório gerado no servidor. Após validação/reencode, mover atomicamente para storage definitivo e marcar `ready`; transações de banco não abrangem atomicamente filesystem, portanto prever reconciliação de arquivos órfãos e entradas pendentes após crash.

Mídia sem referência publicada só é legível por owner/dono. A rota de entrega pública verifica que ao menos um post publicado e não excluído referencia o arquivo; nome imprevisível sozinho não protege rascunho. Usar `nosniff` e Content-Type controlado. Não expor diretório estático bruto do volume.

Remover mídia referenciada retorna 409. Limpeza remove uploads abandonados após 24 horas, conferindo referências novamente para evitar corrida. Ao despublicar, o servidor deixa de autorizar mídia exclusivamente privada; arquivos já baixados não podem ser recolhidos. Nesta fase, usar revalidação obrigatória, sem CDN/cache público duradouro.

Guardar bytes no filesystem persistente e metadados no SQLite. Preparar interface simples de storage para migração futura, sem instalar S3/Redis agora. Backups devem incluir banco e mídia de forma coordenada.

## 10. Contrato HTTP proposto

Base: `/api/v1`. `/health` e `/ready` atuais permanecem operacionais. DTOs e erros continuam em inglês; interface traduz códigos para PT/EN. Os nomes de rotas a seguir são contrato planejado, não endpoints disponíveis.

### 10.1 Autenticação e usuários

| Método e rota | Acesso | Comportamento |
| --- | --- | --- |
| `POST /auth/login` | Público com proteção de origem/rate limit | Login, cookies e perfil seguro |
| `GET /auth/csrf` | Sessão válida | Token CSRF, sem cache |
| `POST /auth/refresh` | Refresh + CSRF | Rotacionar refresh e emitir JWT |
| `POST /auth/logout` | Sessão + CSRF | Revogar sessão e limpar cookies |
| `POST /auth/logout-all` | Autenticado + CSRF | Revogar todas as sessões |
| `GET /users/me` | Autenticado | Perfil próprio |
| `PATCH /users/me` | Autenticado + CSRF | Nome, bio e avatar autorizado |
| `PUT /users/me/password` | Autenticado + senha atual + CSRF | Trocar senha e invalidar sessões |
| `GET /auth/sessions` | Autenticado | Dispositivos/sessões próprias, sem tokens |
| `DELETE /auth/sessions/{id}` | Próprio usuário + CSRF | Revogar sessão específica |
| `GET /admin/users` | Owner | Lista paginada segura, sem hashes |
| `PATCH /admin/users/{id}` | Owner + CSRF + reautenticação | Status/papel; proteger último owner |

Não existem `/register`, `/signup`, `POST /users` ou `POST /admin/users`. Reautenticação de alteração sensível exige verificação de senha atual em payload específico não registrado em logs.

### 10.2 Posts, tags e mídia

| Método e rota | Acesso | Comportamento |
| --- | --- | --- |
| `GET /posts` | Público | Listagem, busca e filtros; somente publicados |
| `GET /posts/{locale}/{slug}` | Público | Detalhe publicado, HTML seguro e autor público |
| `GET /tags` | Público | Somente tags com posts públicos, contagem por locale |
| `GET /media/{id}` | Público ou dono/owner | Verificar publicação/referência ou acesso privado |
| `GET /admin/posts` | Author/owner | Próprios posts ou todos para owner, incluindo estados privados |
| `POST /admin/posts` | Author/owner + CSRF | Criar draft, 201 e Location |
| `GET /admin/posts/{id}` | Dono/owner | Detalhe editável, Markdown e ETag |
| `PATCH /admin/posts/{id}` | Dono/owner + CSRF + If-Match | Atualizar campos e tags atomicamente |
| `DELETE /admin/posts/{id}` | Dono/owner + CSRF + If-Match | Mover para lixeira, 204 |
| `POST /admin/posts/{id}/publish` | Dono/owner + CSRF + If-Match | Validar e publicar |
| `POST /admin/posts/{id}/unpublish` | Dono/owner + CSRF + If-Match | Voltar para draft |
| `POST /admin/posts/{id}/restore` | Dono/owner + CSRF + If-Match | Restaurar como draft |
| `POST /admin/posts/preview` | Author/owner + CSRF | Renderizar sem persistir |
| `POST /admin/posts/import` | Author/owner + CSRF | Importar Markdown como draft |
| `GET /admin/posts/{id}/export` | Dono/owner | Download `.md` |
| `GET /admin/tags` | Author/owner | Catálogo editorial paginado |
| `POST /admin/tags` | Author/owner + CSRF | Criar tag, 409 em slug existente |
| `PATCH /admin/tags/{id}` | Owner + CSRF | Editar nome; slug imutável após uso público |
| `DELETE /admin/tags/{id}` | Owner + CSRF | Apenas sem associações; caso contrário 409 |
| `POST /admin/media` | Author/owner + CSRF | Upload validado |
| `DELETE /admin/media/{id}` | Dono/owner + CSRF | Excluir apenas mídia sem referências |

Respostas de lista: `items`, `next_cursor`, `has_more`; não incluir Markdown/HTML completo. Detalhe público expõe título, descrição, capa/alt, tags, autor público, locale, datas e HTML; conteúdo Markdown bruto fica no contrato editorial. Não aceitar query `include_drafts` no endpoint público, mesmo para usuário autenticado.

Respostas públicas usam ETag e `Cache-Control: no-cache`, exigindo revalidação; verificar visibilidade antes de responder 304. Respostas privadas de mídia/posts usam no-store. Não compartilhar cache de resposta privada com acesso anônimo; testes devem cobrir Cookie/Vary e o caminho público/privado da mídia. CDN com TTL maior fica para uma fase com invalidação explícita.

## 11. Pesquisa, filtros e paginação

`GET /posts`: `q` até 120 caracteres/512 bytes, `locale`, `tag` repetível até 5 valores, `author_id`, `published_from`, `published_to`, `sort`, `limit` e `cursor`. Tags múltiplas usam AND; intervalo UTC é início inclusivo e fim exclusivo. Parâmetros desconhecidos, datas inválidas, intervalo invertido e cursor inválido retornam 400.

Ordenação padrão: `published_desc`; opções `published_asc` e `relevance` somente com `q`. Relevância usa BM25 com maior peso em título, depois descrição, depois corpo; desempate por data e ID. Busca sem texto útil após normalização retorna 400; ausência de `q` faz listagem normal. Tag inexistente resulta lista vazia, nunca remoção silenciosa do filtro.

Limite padrão 20, máximo 50. Cursor usa keyset (data/ID; score/data/ID na busca), inclui identidade dos filtros/ordenação e tem tamanho/tipos validados. Não é segredo nem autorização. Consulta sempre reaplica predicado público e filtros; cursor de outra busca retorna 400. Total exato não é calculado por padrão.

Resultados não formam snapshot entre requisições: publicação/edição pode alterar relevância e paginação. A interface oferece reinício/atualização da busca; estabilidade por ID só garante desempate de conjunto inalterado. No admin, filtrar status, lixeira, autor permitido e `updated_at`, com ordenação igualmente definida.

Usar FTS5 com `unicode61 remove_diacritics 2`, verificando comportamento PT/EN e disponibilidade do recurso no SQLite embarcado. Extrair texto do Markdown sem código HTML e manter só posts públicos em `posts_search`. Publicação, edição, exclusão e despublicação atualizam posts/tags/índice na mesma transação; consulta faz JOIN com posts e repete visibilidade. [Referência técnica FTS5](https://www.sqlite.org/fts5.html).

Não repassar `q` como expressão FTS irrestrita: produzir termos literais escapados, com política AND documentada, sem operadores especiais nem prefixos arbitrários inicialmente. Bind em todos os valores SQL; sort e nomes de colunas vêm de enums fechados. Testar aspas, acentos, pontuação, payloads SQL e termos enormes. Migração faz backfill; CLI oferece rebuild e verificação de consistência do índice.

## 12. Arquitetura Rust e migrations

Organizar por feature: `auth/`, `users/`, `posts/`, `tags/`, `media/` e `audit/`, cada qual com `routes`, `handler`, `dto`, `service`, `repository` e `model` quando necessários. Dividir serviços grandes por caso de uso, como `login`, `refresh`, `publish` e `search`; evitar módulos vazios e abstrações genéricas sem uso.

`auth/password.rs`, `auth/jwt.rs`, `auth/session.rs` e extractor `AuthenticatedUser` isolam segurança. Handlers convertem HTTP/DTO; services coordenam regras/transações; repositories executam SQL parametrizado. Serviços administrativos e seed compartilham regras sem depender de handlers HTTP.

O estado da aplicação passa a conter pool, material JWT, configuração validada, semáforos e storage. Nunca serializar/debugar estado com segredos. Montar router público e router administrativo separados, aplicando autenticação/CSRF no grupo privado com defesa adicional de autorização nos serviços.

Migrations incrementais planejadas: `0002_users`, `0003_auth_sessions`, `0004_media`, `0005_posts_tags`, `0006_posts_search`, `0007_audit_events`. Ajustar dependências de FK, como avatar, por tabela de mídia criada antes de adicionar a referência. Não alterar a migration `0001` já existente.

Migrações devem funcionar em banco novo e no banco atual vazio de domínio. Índice FTS e tabela de junção são recriáveis a partir dos dados canônicos. Fazer backup e verificar integridade antes de mudanças destrutivas; não chamar rollback de aplicação de seguro quando schema novo é incompatível.

Proposta de bibliotecas adicionais: `jsonwebtoken`, `argon2`, `secrecy`/`zeroize`, `rpassword`, `pulldown-cmark`, `ammonia`, `image` e suporte de cookies compatível com Axum. Aleatoriedade e comparação constante usam bibliotecas mantidas. Selecionar versões/features com MSRV, licenças, RustSec e tamanho de dependências verificados; nenhuma será instalada nesta etapa.

## 13. Limites, erros e observabilidade

Preservar envelope `error.code`/`error.message`, acrescentando `request_id` e erros de campos seguros quando úteis. Sem SQL interno, paths, conteúdo de post privado, valores de senha ou detalhes criptográficos em respostas.

Códigos: 400 consulta/JSON inválido; 401 credencial/sessão inválida; 403 CSRF/papel proibido; 404 invisível/inexistente; 409 unicidade/estado/referência; 412 versão antiga; 413 payload; 415 tipo; 422 validação; 428 If-Match ausente; 429 limite; 503 indisponibilidade transitória. Não retryar mutações automaticamente quando o resultado do commit é desconhecido.

A base atual limita JSON a 64 KiB. Propor limites por rota: auth/perfil 16 KiB, posts/preview 512 KiB de JSON, Markdown importado 256 KiB, upload 6 MiB. Garantir que camada global/proxy não contradiga esses valores; contar streaming. Manter 10 segundos para rotas normais e configuração específica de 30 segundos para upload.

Auditoria: seed, login, refresh reutilizado, logout, senha, papéis/status, criação/edição/publicação/exclusão/restauração. Registrar IDs e resultado; nunca Authorization, Cookie, Set-Cookie, senha, refresh, CSRF, hash de senha ou corpos. Falhas de login públicas não devem causar escrita ilimitada: agregar/amostrar eventos e limitar volume.

Retenção inicial de auditoria: 90 dias, com limpeza em lotes pequenos; evitar IP completo quando desnecessário. Métricas com rótulos de baixa cardinalidade: latência, falhas de auth, 429, saturação de hashing, SQLite busy, disco e sucesso de backup. Não usar e-mail/IP/IDs como labels de métricas.

SQLite: transações de escrita curtas, nenhum hash/render/upload dentro delas, WAL/FULL mantidos, pool pequeno e nenhum Redis obrigatório. Benchmark no alvo real de login, busca, render e consumo de memória antes de alterar parâmetros.

## 14. Planejamento da experiência web

Área `/admin` com login, lista editorial, editor, tags, mídia e perfil/sessões. A proteção de rota no frontend melhora UX; autorização efetiva é sempre no backend. Não existe botão/tela de cadastro.

Criar componentes no Design System antes do uso: campos, alertas, tabela/lista paginada, seletor de tags, upload, editor Markdown, preview, diálogo de confirmação e indicador de salvamento. Todo fluxo possui loading, vazio, sucesso e erro, com textos PT/EN e acessibilidade por teclado.

Editor mantém alterações locais em memória, avisa ao sair sem salvar, mostra status de publicação e conflito 412 com opção de recarregar/copiar texto. Autosave fica desativado inicialmente para não sobrescrever edição publicada nem introduzir armazenamento privado persistente no navegador.

Sessão expirada tenta refresh uma vez de forma coordenada; falha preserva texto em memória e solicita login. Não entrar em loops de retry. Busca pública sincroniza filtros na URL, cancela requisições obsoletas e usa debounce; paginação mantém feedback de carregamento e resultado vazio explícito.

SEO: title/description/canonical/Open Graph do conteúdo público, imagem/alt, data de publicação/atualização e sitemap de publicados. Renderização indexável/SSR deve ser avaliada no frontend existente antes de prometer indexação; páginas privadas usam noindex. Sanitização também é aplicada a metadados e atributos HTML.

## 15. Testes e critérios de aceite

| Área | Casos obrigatórios |
| --- | --- |
| Seed | Banco novo, repetição sem reset, dois seeds concorrentes, owner já existente, entrada ausente, nenhum segredo em stdout/stderr |
| Usuários | E-mail único/normalização, último owner, inativação, campos privados nunca serializados, nenhuma rota de registro |
| JWT | Assinatura/algoritmo/chave inválidos, claim ausente, expiração, issuer/audience errados, clock skew e revogação |
| Sessões | Rotação atômica, replay, duas abas, resposta perdida, logout com acesso expirado, logout-all, reset e expiração absoluta |
| CSRF | Origin ausente/hostil/null, token ausente/incorreto, login cross-site, multipart e refresh protegidos |
| Autorização | Anônimo não escreve; autor não lê draft/edita post/mídia alheios; owner administra sem violar último owner |
| Posts | CRUD, slug único/imutável, estados, lixeira/restore, If-Match, 412, rollback de tags/FTS e mídia referenciada |
| Público | Draft/lixeira nunca aparecem em detalhe, lista, busca, tags, contagens, sitemap ou arquivos de mídia |
| Markdown | Scripts, HTML, esquemas perigosos, links, código, Unicode, import/export e preview idêntico ao HTML salvo |
| Upload | Tipo forjado, SVG, path traversal, excesso de bytes/pixels, memória limitada, crash entre arquivo e commit |
| Busca | Acentos PT/EN, aspas, AND de tags, paginação/desempate, filtro de autor/data/idioma e FTS após alterações |
| Operação | Migrations antigas/novas, backup+restore com mídia, startup sem chave, rotação JWT, disco cheio e pool indisponível |

Testes usam credenciais fictícias geradas por execução, bancos temporários e relógio controlável. Nenhum teste consulta segredo real. Adicionar E2E de login → draft → upload → preview → publish → consulta anônima → editar → despublicar → logout.

Gate por etapa: `cargo fmt`, Clippy sem warnings, testes pertinentes, checagem de arquivos com até 500 linhas, análise de dependências e revisão de segredos/diff. Revisão de auth deve verificar ameaças e respostas negativas, não apenas happy path. O agente validador definido em `agents/validator.md` deverá revisar cada etapa da implementação futura antes de considerá-la concluída.

## 16. Ordem de implementação e entregáveis

1. Definir contratos OpenAPI, DTOs, estados editoriais, erros e configuração de segredos/origin; validar alinhamento com o frontend.
2. Criar migrations de usuários/sessões e serviços de senha; entregar CLI de seed/recuperação com testes de idempotência e concorrência.
3. Implementar JWT, login, cookies, CSRF, refresh, revogação e rate limits; concluir testes adversariais antes de expor escrita.
4. Implementar perfil, sessões próprias e administração limitada; validar ausência de cadastro e proteção do último owner.
5. Implementar armazenamento de mídia e pipeline Markdown seguros, com reconciliação de arquivos e limites de recursos.
6. Implementar posts/tags, autorização, publicação, lixeira, import/export e concorrência otimista em transações.
7. Implementar consulta pública, FTS, filtros, paginação, DTOs públicos e sitemap; testar vazamento de rascunhos em todas as superfícies.
8. Construir componentes do Design System e área web; integrar sessão, editor, upload, acessibilidade e PT/EN.
9. Validar operação: secret injection, backup/restauração, métricas, índices, latência/memória e build de produção sem seed secrets.
10. Revisar com o validador e dividir commits por responsabilidade; executar seed real somente em operação explícita posterior, fora de Git/build.

Aceite final: um proprietário provisionado de forma privada consegue publicar pela web; visitantes acessam somente posts publicados; nenhuma rota cria usuários; nenhuma credencial integra fonte, artefato ou logs; cada escrita verifica sessão, permissão e validade da entrada.

## 17. Pontos a confirmar antes da implementação

Defaults deste plano permitem prosseguir sem redesenhar a base. Confirmar antes do deploy: domínio/origin definitivo, host com volume persistente, orçamento de disco/memória, destino criptografado dos backups e responsável pela recuperação.

Confirmar preferências de produto antes de ampliar escopo: publicação imediata de edições, capa obrigatória, idioma padrão, permissão de authors para publicar sem aprovação, retenção da lixeira e necessidade de MFA. Enquanto não houver decisão diferente, valem os defaults explícitos deste documento.

Não foram implementados código, migrations, contas, segredos, commits ou deploy por este planejamento. A implementação futura deve revalidar versões e recomendações das fontes vinculadas, que podem evoluir.
