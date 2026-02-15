# Finding Data

> **USING THE API**

How do you find data on TMDB?

There are 3 ways to search for and find movies, TV shows and people on TMDB. They're outlined below.

- **`/search`** - Text based search is the most common way. You provide a query string and we provide the closest match. Searching by text takes into account all original, translated, alternative names and titles.
- **`/discover`** - Sometimes it useful to search for movies and TV shows based on filters or definable values like ratings, certifications or release dates. The discover method make this easy.
- **`/find`** - The last but still very useful way to find data is with existing external IDs. For example, if you know the IMDB ID of a movie, TV show or person, you can plug that value into this method and we'll return anything that matches. This can be very useful when you have an existing tool and are adding our service to the mix.

Take a look at the [Search & Query for Details](https://developer.themoviedb.org/docs/search-and-query-for-details) page for some basic workflows you might use to search and query data.
