# Append To Response

> **USING THE API**

`append_to_response` is an easy and efficient way to append extra requests to any top level namespace.

The movie, TV show, TV season, TV episode and person detail methods all support a query parameter called `append_to_response`. This makes it possible to make sub requests within the same namespace in a single HTTP request. Each request will get appended to the response as a new JSON object.

Here's a quick example, let's assume you want the movie details and the videos for a movie. Usually you would think you have to issue two requests:

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/11' \\
  --header 'Authorization: Bearer <<access_token>>'

curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/11/videos' \\
  --header 'Authorization: Bearer <<access_token>>'
```

But with `append_to_response` you can issue a single request:

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/11?append_to_response=videos' \\
  --header 'Authorization: Bearer <<access_token>>'
```

Even more powerful, you can issue multiple requests, just comma separate the values:

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/11?append_to_response=videos,images' \\
  --header 'Authorization: Bearer <<access_token>>'
```

> **📘 Note**
> Each method will still respond to whatever query parameters are supported by each individual request. This is worth pointing out specifically for images since your language parameter will filter images. This is where the `include_image_language` parameter can be useful as outlined in the image language page.
