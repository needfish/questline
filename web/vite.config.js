export default {
  server: { proxy: { "/items": "http://127.0.0.1:8080", "/capture": "http://127.0.0.1:8080", "/reason": "http://127.0.0.1:8080" } },
};
