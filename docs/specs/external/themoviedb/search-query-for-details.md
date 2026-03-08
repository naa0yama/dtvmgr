# Search & Query For Details

> **USING THE API**

Learn how to search and query for a movie.

A common workflow here on TMDB is to search for a movie (or TV show, or person) and then query for the details. Here's a quick overview of what that flow looks like.

### Search

First, you are going to issue a query to one of the movie, TV show or person search methods. We'll use Jack Reacher and the movie method for this example:

**Example Search Request:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/search/movie?query=Jack+Reacher' \\
  --header 'Authorization: Bearer <<access_token>>'
```

This will return a few fields, the one you want to look at is the `results` field. This is an array and will contain our standard movie list objects. Here's an example of the first item:

**Example Results Object:**

```json
{
	"poster_path": "/IfB9hy4JH1eH6HEfIgIGORXi5h.jpg",
	"adult": false,
	"overview": "Jack Reacher must uncover the truth behind a major government conspiracy in order to clear his name. On the run as a fugitive from the law, Reacher uncovers a potential secret from his past that could change his life forever.",
	"release_date": "2016-10-19",
	"genre_ids": [53, 28, 80, 18, 9648],
	"id": 343611,
	"original_title": "Jack Reacher: Never Go Back",
	"original_language": "en",
	"title": "Jack Reacher: Never Go Back",
	"backdrop_path": "/4ynQYtSEuU5hyipcGkfD6ncwtwz.jpg",
	"popularity": 26.818468,
	"vote_count": 201,
	"video": false,
	"vote_average": 4.19
}
```

### Query For Details

With the item above in hand, you can see the `id` of the movie is `343611`. You can use that id to query the movie details method:

**Example Details Query:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/343611' \\
  --header 'Authorization: Bearer <<access_token>>'
```

This will return all of the main movie details as outlined in the movie details documentation.

I would also suggest taking a read through the [Append To Response](https://developer.themoviedb.org/docs/append-to-response) document as it outlines how you can make multiple sub requests in one. For example, with videos:

**Example Append Request:**

```bash
curl --request GET \\
  --url 'https://api.themoviedb.org/3/movie/11?append_to_response=videos' \\
  --header 'Authorization: Bearer <<access_token>>'
```
