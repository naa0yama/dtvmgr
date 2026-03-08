# JSON & JSONP

> **USING THE API**

The only response format we support is JSON.

If you are using a JavaScript library and need to make requests from another public domain, you can use the `callback` parameter which will encapsulate the JSON response in a JavaScript function for you.

### Example JSONP Request

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/search/movie?query=Batman&callback=test' \\
  --header 'Authorization: Bearer <<access_token>>' \\
  --header 'accept: application/json'
```
