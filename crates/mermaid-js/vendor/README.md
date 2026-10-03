`mermaid.min.js` and `LICENSE` are `dist/mermaid.min.js` and `LICENSE` of the npm package
`mermaid` 11.17.2 (MIT), unchanged:

```sh
curl -sSLO https://registry.npmjs.org/mermaid/-/mermaid-11.17.2.tgz
tar xzf mermaid-11.17.2.tgz package/dist/mermaid.min.js package/LICENSE
sha256sum package/dist/mermaid.min.js
# 581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8
```

To move to another version, replace both files the same way and change `version!` in
`src/lib.rs`; a test checks that the two agree.
