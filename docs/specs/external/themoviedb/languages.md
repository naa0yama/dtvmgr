# Languages

> **USING THE API**

Learn about languages on TMDB.

TMDB tries to be localized wherever possible. While most of our metadata endpoints support translated data, there are still a few gaps that do not. The two main areas that are not are person names and characters. We're working to support this.

### ISO 639-1

The language code system we use is ISO 639-1. Unfortunately, there are a number of languages that don't have a ISO-639-1 representation. We may decide to upgrade to ISO-639-3 in the future but do not have any immediate plans to do so.

### ISO 3166-1

You'll usually find our language codes mated to a country code in the format of `en-US`. The country codes in use here are ISO 3166-1.

Now that you know how languages work, let's look at some example requests.

**English Example:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/tv/1399?language=en-US' \\
  --header 'Authorization: Bearer <<access_token>>' \\
  --header 'accept: application/json'
```

**Portuguese Example:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/popular?language=pt-BR' \\
  --header 'Authorization: Bearer <<access_token>>' \\
  --header 'accept: application/json'
```
