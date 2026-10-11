/* ***************************************************
 * printpwd
 * This utility extracts a password from the parameter
 * file and prints it in an operator friendly way.
 * ***************************************************
 */

#include <stdio.h>

char match[] = "Password" ;

main( unsigned argc, char *argv[] )
{
	int i, j, found_count, len, skip_first;
	FILE *infile;
	char inbuf[128], password[128];
	char *ptr;
	
	if( argc < 2 )
	{
		fprintf( stderr, "Error - Usage is :\nprintpwd filename [ header... ]\n\n");
		fprintf( stderr, "This program is used to extract the password from a\n");
		fprintf( stderr, "TheNet X-series parameter save file and format it into\n");
		fprintf( stderr, "a table suitable for operator use.\n");
		fprintf( stderr, "It sends its output to stdout, so it may be redirected\n");
		fprintf( stderr, "to a file or device. Errors are sent to stderr\n");
		fprintf( stderr, "Return codes > 0 correspond to errors.\n" );
		exit(1);
	}
	if( (infile = fopen( argv[1], "r" ) ) == NULL )
	{
		fprintf( stderr, "Error - Cannot open file '%s' for reading\n", argv[1] );
		exit(2);
	}
	found_count = 0;
	while( !feof( infile ) && !ferror( infile ) )
	{
		if( fgets( inbuf, sizeof(inbuf), infile ) != NULL )
		{
			if( strncmp( match, inbuf, strlen( match) ) == 0 )
			{
				ptr = inbuf + strlen( match );
				while( *ptr && *ptr != '=' )
					ptr++;
				if( *ptr == '=' )
					ptr++;
				while( *ptr == ' ' )
					ptr++;
				strcpy( password, ptr );
				found_count++;
			}
		}
	}
	if( ferror( infile ) )
	{
		fprintf( stderr, "Error in reading from input file.\n" );
		exit( 3 );
	}
	fclose( infile );

	if( found_count == 0 )
	{
		fprintf( stderr, "Error - No password entry found in file.\n" );
		exit( 4 );
	}
	if( found_count > 1 )
		fprintf( stderr, "Warning - Multiple passwords in file. Last one used.\n");

	for( len = 0, i = 2; i < argc; i++ )
		len += strlen( argv[i] ) + 1;
	i = ( len < 80 ) ? ( 80 - len ) / 2 : 0;
	while( i-- )
		fputc( ' ', stdout );
	for( i = 2; i < argc; i++ )
		fprintf( stdout, "%s ", argv[i]);

	fputs( "\n\n\n\n\n                    x0  x1  x2  x3  x4  x5  x6  x7  x8  x9\n", stdout );
	ptr = password;
	skip_first = 0;
	for( i=0; i<=8; i++ )
	{
		fprintf(stdout, "\n                 %dx ", i );
		for( j=0; j<10; j++ )
		{
			if( skip_first && *ptr != '\0' )
				fprintf( stdout, " %c  ", *ptr++ );
			else
				fputs( "    ", stdout );
			skip_first = 1;
		}
		fputs("\n", stdout );
	}
	return( 0 );
}

		