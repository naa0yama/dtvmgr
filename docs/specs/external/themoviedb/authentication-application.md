# Authentication - Application

> **AUTHENTICATION**

The default way to authenticate.

Application level authentication would generally be considered the default way of authenticating yourself on the API. Version 3 is controlled by either a single query parameter, `api_key`, or by using your access token as a Bearer token.

You can request an API key by logging in to your account on TMDB and clicking [here](https://www.themoviedb.org/settings/api).

### Bearer Token

The default method to authenticate is with your access token. If you head into your account page, under the API settings section, you will see a new token listed called **API Read Access Token**. This token is expected to be sent along as an Authorization header.

A simple cURL example using this method looks like the following:

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/11' \\
  --header 'Authorization: Bearer <<access_token>>'
```

Using the Bearer token has the added benefit of being a single authentication process that you can use across both the v3 and v4 methods. Both authentication methods provide the same level of access, and which one you choose is completely up to you.
