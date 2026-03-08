## Regions

> **USING THE API**

How does region support work on TMDB?

There's two aspects to regions on TMDB that should be explained. The first is the new `region` parameter.

The `region` parameter will act as a filter to search for and display matching release date information. This parameter is expected to be an ISO 3166-1 code.

For example, if you were searching for the movie Whiplash and wanted to show the German release date (to go with the German translation) you can make a query like so:

**Example German Request:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/search/movie?query=Whiplash&language=de-DE&region=DE' \\
  --header 'Authorization: Bearer <<access_token>>' \\
  --header 'accept: application/json'
```

In this case, `region` is simply acting as a presentation filter. In the event that we don't have a release date entered for the country you are searching for, we simply default back to the primary release date like always. This is the same as entering no region parameter.

Where this can get pretty amazing is with the `with_release_type` filter that can work in tandem with the `region`. Let's say you were looking for movies that are in the theatres in Germany this week. That's easy, we can now build this query:

**Example German Theatrical Request:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/discover/movie?language=de-DE&region=DE&release_date.gte=2016-11-16&release_date.lte=2016-12-02&with_release_type=2|3' \\
  --header 'Authorization: Bearer <<access_token>>' \\
  --header 'accept: application/json'
```

You can of course specify any release type as found in our documentation. If you do not specify `with_release_type` while using the `region` param on discover, `region` simply acts as a filter looking for any movies that match your filter criteria that has at a minimum, one matching release date for the country specified.

### Release Types

| Type | Release              |
| ---: | -------------------- |
|    1 | Premiere             |
|    2 | Theatrical (limited) |
|    3 | Theatrical           |
|    4 | Digital              |
|    5 | Physical             |
|    6 | TV                   |
